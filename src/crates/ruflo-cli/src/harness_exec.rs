//! Harness behavioral layer — loop, verify, replay, canary.
//!
//! Ports the behavioral core of the 15 TS harness-* services. The state
//! layer lives in services::harness; this module adds real execution:
//!
//! - verify::run(repo): runs `cargo test`, parses pass/fail counts, returns a
//!   structured verdict (the highest-value harness behavior).
//! - r#loop::run(repo, max_iters): orchestrate verify → if failing, spawn a
//!   headless worker to fix → re-verify. Bounded iterations.
//! - replay::run(run_id): replay a recorded harness run from state.
//! - canary::check(repo): run verify; block (return gating=false) if the
//!   canary metric (pass rate) drops below threshold.

use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Run `cargo test` in `repo` and parse the result into a verdict.
pub fn verify(repo: &Path) -> Value {
    let out = Command::new("cargo")
        .args(["test", "--quiet", "--no-fail-fast"])
        .current_dir(repo)
        .output();
    let (ok, passed, failed, stdout, stderr) = match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            // Parse "test result: ok. N passed; M failed;" lines.
            let mut passed = 0u64;
            let mut failed = 0u64;
            for line in text.lines() {
                if line.starts_with("test result:") {
                    if let Some(p) = parse_count(line, "passed") { passed += p; }
                    if let Some(f) = parse_count(line, "failed") { failed += f; }
                }
            }
            (o.status.success(), passed, failed,
             text.to_string(), String::from_utf8_lossy(&o.stderr).to_string())
        }
        Err(e) => (false, 0, 0, String::new(), e.to_string()),
    };
    let verdict = if ok { "pass" } else if failed > 0 { "fail" } else { "error" };
    let entry = json!({
        "verdict": verdict, "ok": ok,
        "passed": passed, "failed": failed,
        "stdout": stdout.chars().take(4000).collect::<String>(),
        "stderr": stderr.chars().take(1000).collect::<String>(),
        "at": now_ms(),
    });
    // Record into the harness-verify state.
    crate::services::harness::record_run("verify", entry.clone());
    entry
}

fn parse_count(line: &str, key: &str) -> Option<u64> {
    let pat = format!("{key};");
    let idx = line.find(&pat)?;
    // Walk back from idx over digits + spaces to find the number.
    let before = &line[..idx];
    let digits: String = before.chars().rev()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().ok()
}

/// Orchestrate the verify→fix→verify loop. Each failing iteration spawns a
/// headless worker (claude -p) to fix the failing tests, then re-verifies.
/// Stops when verify passes or max_iters is hit.
pub fn run_loop(repo: &Path, max_iters: usize) -> Value {
    let mut iters = Vec::new();
    let mut final_verdict = "fail";
    for i in 0..max_iters.max(1) {
        let v = verify(repo);
        let verdict = v["verdict"].as_str().unwrap_or("error");
        iters.push(json!({"iter": i, "verdict": verdict,
            "passed": v["passed"], "failed": v["failed"]}));
        if verdict == "pass" {
            final_verdict = "pass";
            break;
        }
        // Spawn a fix attempt via the headless executor.
        let prompt = format!(
            "Tests are failing ({} failed). Read the test output, find the root cause, and fix it. Make minimal changes.",
            v["failed"].as_u64().unwrap_or(0)
        );
        let _ = crate::services::headless::execute(
            "harness-fix", "claude", &prompt, 120_000, &[],
        );
    }
    let result = json!({
        "verdict": final_verdict,
        "iterations": iters.len(),
        "trace": iters,
        "at": now_ms(),
    });
    crate::services::harness::record_run("loop", result.clone());
    result
}

/// Replay a recorded harness run by id (looks across all harness-<type> state).
pub fn replay(run_id: &str) -> Value {
    let types = crate::services::harness::HARNESS_TYPES;
    for t in types {
        let runs = crate::services::harness::list_runs(t);
        for r in runs {
            if r["id"].as_str() == Some(run_id) {
                return json!({"found": true, "type": t, "run": r});
            }
        }
    }
    json!({"found": false, "runId": run_id})
}

/// Canary gate: run verify; if pass-rate < threshold, block promotion.
pub fn canary(repo: &Path, threshold: f64) -> Value {
    let v = verify(repo);
    let passed = v["passed"].as_u64().unwrap_or(0);
    let failed = v["failed"].as_u64().unwrap_or(0);
    let total = passed + failed;
    let rate = if total == 0 { 0.0 } else { passed as f64 / total as f64 };
    let gate = if total == 0 { false } else { rate >= threshold };
    let result = json!({
        "gate": gate, "passRate": rate, "threshold": threshold,
        "passed": passed, "failed": failed,
        "verdict": if gate { "release" } else { "block" },
    });
    crate::services::harness::record_run("canary", result.clone());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_count_extracts_passed() {
        assert_eq!(parse_count("test result: ok. 12 passed; 0 failed;", "passed"), Some(12));
        assert_eq!(parse_count("test result: ok. 12 passed; 3 failed;", "failed"), Some(3));
    }

    #[test]
    fn verify_returns_verdict() {
        // cargo test in this repo (or whatever cwd the test runs in).
        let v = verify(Path::new("."));
        let verdict = v["verdict"].as_str().unwrap_or("");
        assert!(verdict == "pass" || verdict == "fail" || verdict == "error");
        assert!(v["ok"].is_boolean());
    }

    #[test]
    fn canary_blocks_low_pass_rate() {
        let r = canary(Path::new("."), 1.1); // impossible threshold → always block
        assert_eq!(r["gate"].as_bool(), Some(false));
        assert_eq!(r["verdict"].as_str(), Some("block"));
    }

    #[test]
    fn replay_missing_returns_not_found() {
        let r = replay("definitely-nonexistent-run-id-xyz");
        assert_eq!(r["found"].as_bool(), Some(false));
    }
}
