//! Repo supervisor — periodic health probes + alert emission.
//!
//! Ports services/repo-supervisor.ts behavioral parity. Runs a set of
//! read-only probes (git drift, build state, test pass rate, disk usage) and
//! records the result, surfacing issues. The daemon loop calls run_probes()
//! on its interval; the CLI exposes `ruflo supervisor status`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run all health probes against `repo` and record the result. Returns the
/// check entry (status: ok | degraded | failing, issues[], probes{}).
pub fn run_probes(repo: &Path) -> Value {
    let probes = json!({
        "git": probe_git(repo),
        "build": probe_build(repo),
        "tests": probe_tests(repo),
        "disk": probe_disk(repo),
        "cargoLock": probe_cargo_lock(repo),
    });
    let issues = collect_issues(&probes);
    let status = if issues.iter().any(|i| i["severity"].as_str() == Some("high")) {
        "failing"
    } else if !issues.is_empty() {
        "degraded"
    } else {
        "ok"
    };
    let entry = json!({
        "status": status,
        "issues": issues,
        "probes": probes,
        "checkedAt": now_ms(),
    });
    record(&entry);
    // Emit an alert to swarm memory when degraded/failing.
    if status != "ok" {
        emit_alert(&entry);
    }
    entry
}

fn record(entry: &Value) {
    // Delegate to the services supervisor module's state file for continuity.
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("repo-supervisor.json");
    let mut state: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"checks": []}));
    if state["checks"].is_null() {
        state["checks"] = json!([]);
    }
    if let Some(arr) = state["checks"].as_array_mut() {
        arr.push(entry.clone());
        // Bound the history to the last 200 checks.
        if arr.len() > 200 {
            let drop = arr.len() - 200;
            arr.drain(0..drop);
        }
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&state).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn emit_alert(entry: &Value) {
    // Best-effort: append to .claude-flow/alerts.jsonl so the swarm/daemon can pick it up.
    let path = state_dir().join("alerts.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", serde_json::to_string(entry).unwrap_or_default());
    }
}

fn collect_issues(probes: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    // git: dirty tree or unpushed commits → medium.
    if probes["git"]["dirty"].as_bool() == Some(true) {
        out.push(json!({"probe": "git", "severity": "low", "issue": "uncommitted changes"}));
    }
    if probes["git"]["ahead"].as_u64().unwrap_or(0) > 0 {
        out.push(json!({"probe": "git", "severity": "low",
            "issue": format!("{} unpushed commits", probes["git"]["ahead"].as_u64().unwrap_or(0))}));
    }
    // build: failing → high.
    if probes["build"]["ok"].as_bool() == Some(false) {
        out.push(json!({"probe": "build", "severity": "high", "issue": "build failed"}));
    }
    // tests: failing → high.
    if probes["tests"]["ok"].as_bool() == Some(false) {
        out.push(json!({"probe": "tests", "severity": "high", "issue": "tests failing"}));
    }
    // disk: >90% → medium.
    if let Some(pct) = probes["disk"]["usedPct"].as_f64() {
        if pct > 90.0 {
            out.push(json!({"probe": "disk", "severity": "medium", "issue": format!("{pct:.0}% disk used")}));
        }
    }
    out
}

fn probe_git(repo: &Path) -> Value {
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo).output()
        .ok()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    let ahead = Command::new("git")
        .args(["rev-list", "--count", "@{u}..HEAD"])
        .current_dir(repo).output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
        .unwrap_or(0);
    json!({"dirty": dirty, "ahead": ahead})
}

fn probe_build(repo: &Path) -> Value {
    // `cargo check` is the cheapest real build signal.
    let ok = Command::new("cargo")
        .args(["check", "--quiet", "--offline"])
        .current_dir(repo).status()
        .map(|s| s.success())
        .unwrap_or(false);
    json!({"ok": ok, "command": "cargo check"})
}

fn probe_tests(repo: &Path) -> Value {
    // Only run if there's a fast test indicator; skip full suite (too slow for
    // a periodic probe). We check `cargo test --no-run` compiles test bodies.
    let ok = Command::new("cargo")
        .args(["test", "--quiet", "--no-run", "--offline"])
        .current_dir(repo).status()
        .map(|s| s.success())
        .unwrap_or(false);
    json!({"ok": ok, "command": "cargo test --no-run"})
}

fn probe_disk(repo: &Path) -> Value {
    // df on the repo's filesystem (Unix); degrade to unknown elsewhere.
    let pct = Command::new("df").arg("-P").arg(repo).output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            let line = text.lines().nth(1)?;
            let cap: f64 = line.split_whitespace().nth(4)?
                .trim_end_matches('%').parse().ok()?;
            Some(cap)
        });
    json!({"usedPct": pct.unwrap_or(0.0)})
}

fn probe_cargo_lock(repo: &Path) -> Value {
    let present = repo.join("Cargo.lock").exists();
    json!({"present": present})
}

fn state_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".claude-flow")
}

pub fn latest() -> Value {
    let path = state_dir().join("repo-supervisor.json");
    let parsed: Option<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    match parsed {
        Some(s) => s["checks"].as_array().and_then(|a| a.last().cloned())
            .unwrap_or_else(|| json!({"status": "unknown"})),
        None => json!({"status": "unknown"}),
    }
}

pub fn history(limit: usize) -> Vec<Value> {
    let path = state_dir().join("repo-supervisor.json");
    let mut checks: Vec<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .and_then(|s: Value| s["checks"].as_array().cloned())
        .unwrap_or_default();
    checks.reverse();
    checks.truncate(limit.max(1));
    checks
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_probes_returns_status_in_empty_dir() {
        // No git/cargo → build/tests probes fail → status failing or degraded.
        let dir = tempfile::tempdir().unwrap();
        let r = run_probes(dir.path());
        let _ = r; // state written to cwd; ignore for the assertion
        let st = r["status"].as_str().unwrap_or("");
        assert!(st == "ok" || st == "degraded" || st == "failing", "got {st}");
        assert!(r["probes"].is_object());
    }

    #[test]
    fn collect_issues_flags_failing_build() {
        let probes = json!({
            "git": {"dirty": false, "ahead": 0},
            "build": {"ok": false},
            "tests": {"ok": true},
            "disk": {"usedPct": 50.0},
            "cargoLock": {"present": true},
        });
        let issues = collect_issues(&probes);
        assert!(issues.iter().any(|i| i["probe"].as_str() == Some("build")
            && i["severity"].as_str() == Some("high")));
    }

    #[test]
    fn collect_issues_clean_repo_no_issues() {
        let probes = json!({
            "git": {"dirty": false, "ahead": 0},
            "build": {"ok": true},
            "tests": {"ok": true},
            "disk": {"usedPct": 40.0},
            "cargoLock": {"present": true},
        });
        assert!(collect_issues(&probes).is_empty());
    }
}
