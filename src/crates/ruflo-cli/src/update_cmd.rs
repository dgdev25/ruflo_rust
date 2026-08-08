//! Native V3 `update` command (ADR-025) — npm package update system.
//!
//! Source: `v3/@claude-flow/cli/src/commands/update.ts`. Subcommands:
//! check/all/history/rollback/clear-cache. Uses npm CLI subprocess for real
//! registry calls + durable history.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn history_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/update-history.json")
}

fn load_history(root: &Path) -> Vec<Value> {
    fs::read_to_string(history_file(root))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default()
}

fn save_history(root: &Path, entries: &[Value]) -> bool {
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = history_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(entries) else {
        return false;
    };
    fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, &path).is_ok()
}

fn installed_version(pkg: &str) -> Option<String> {
    Command::new("npm")
        .args(["ls", pkg, "--depth=0", "--json"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let v: Value = serde_json::from_slice(&o.stdout).ok()?;
            v.get("dependencies")?
                .get(pkg)?
                .get("version")
                .and_then(Value::as_str)
                .map(String::from)
        })
}

fn latest_version(pkg: &str) -> Option<String> {
    Command::new("npm")
        .args(["view", pkg, "version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCommand {
    pub operation: String,
    pub json: bool,
    pub force: bool,
    pub dry_run: bool,
    pub include_major: bool,
    pub limit: usize,
    pub clear: bool,
}

pub fn run(root: &Path, command: UpdateCommand) -> u8 {
    match command.operation.as_str() {
        "check" => check(root, &command),
        "all" => all(root, &command),
        "history" => history(root, &command),
        "rollback" => rollback(root, &command),
        "clear-cache" => {
            let cache = root.join(".claude-flow/update-cache.json");
            let _ = fs::remove_file(&cache);
            println!("Update cache cleared.");
            0
        }
        _ => {
            eprintln!("[ERROR] Unknown operation: {}", command.operation);
            eprintln!("  Valid: check, all, history, rollback, clear-cache");
            1
        }
    }
}

const PACKAGES: &[&str] = &[
    "@claude-flow/cli",
    "@claude-flow/cli-core",
    "@claude-flow/security",
    "@claude-flow/guidance",
];

fn check(_root: &Path, command: &UpdateCommand) -> u8 {
    let mut results = Vec::new();
    for pkg in PACKAGES {
        let installed = installed_version(pkg);
        let latest = latest_version(pkg);
        let update_available = match (&installed, &latest) {
            (Some(i), Some(l)) => i != l,
            _ => false,
        };
        let entry = json!({
            "package": pkg,
            "installed": installed,
            "latest": latest,
            "updateAvailable": update_available,
        });
        results.push(entry);
    }
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(results)).unwrap_or_default()
        );
    } else if results.is_empty() {
        println!("No packages found.");
    } else {
        for r in &results {
            let pkg = r["package"].as_str().unwrap_or("?");
            let inst = r["installed"].as_str().unwrap_or("not installed");
            let latest = r["latest"].as_str().unwrap_or("?");
            let avail = if r["updateAvailable"].as_bool() == Some(true) {
                "  → update available"
            } else {
                ""
            };
            println!("  {pkg}: {inst} (latest: {latest}){avail}");
        }
    }
    0
}

fn all(root: &Path, command: &UpdateCommand) -> u8 {
    let mut updated = Vec::new();
    for pkg in PACKAGES {
        let installed = installed_version(pkg);
        let latest = latest_version(pkg);
        let needs_update = match (&installed, &latest) {
            (Some(i), Some(l)) => i != l,
            _ => false,
        };
        if !needs_update || command.dry_run {
            continue;
        }
        // Execute npm update
        let status = Command::new("npm")
            .args(["install", &format!("{pkg}@latest"), "-g"])
            .status();
        let entry = json!({
            "package": pkg,
            "from": installed,
            "to": latest,
            "success": status.map(|s| s.success()).unwrap_or(false),
        });
        updated.push(entry);
    }
    if !updated.is_empty() {
        let mut hist = load_history(root);
        hist.extend(updated.iter().cloned());
        save_history(root, &hist);
    }
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(updated)).unwrap_or_default()
        );
    } else if updated.is_empty() {
        println!("All packages up to date.");
    } else {
        for r in &updated {
            let pkg = r["package"].as_str().unwrap_or("?");
            let from = r["from"].as_str().unwrap_or("?");
            let to = r["to"].as_str().unwrap_or("?");
            let ok = if r["success"].as_bool() == Some(true) {
                "OK"
            } else {
                "FAILED"
            };
            println!("  [{ok}] {pkg}: {from} → {to}");
        }
    }
    0
}

fn history(root: &Path, command: &UpdateCommand) -> u8 {
    if command.clear {
        save_history(root, &[]);
        println!("Update history cleared.");
        return 0;
    }
    let hist = load_history(root);
    let limited: Vec<_> = hist.iter().rev().take(command.limit).collect();
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(limited)).unwrap_or_default()
        );
    } else if limited.is_empty() {
        println!("No update history.");
    } else {
        for (i, r) in limited.iter().enumerate() {
            let pkg = r["package"].as_str().unwrap_or("?");
            let from = r["from"].as_str().unwrap_or("?");
            let to = r["to"].as_str().unwrap_or("?");
            println!("  {}. {pkg}: {from} → {to}", i + 1);
        }
    }
    0
}

fn rollback(root: &Path, command: &UpdateCommand) -> u8 {
    let hist = load_history(root);
    let last = hist.last();
    let Some(last) = last else {
        eprintln!("[ERROR] No update history to roll back.");
        return 1;
    };
    let pkg = last["package"].as_str().unwrap_or("");
    let from = last["from"].as_str().unwrap_or("");
    if command.dry_run {
        println!("Would roll back {pkg} to {from}");
        return 0;
    }
    let status = Command::new("npm")
        .args(["install", &format!("{pkg}@{from}"), "-g"])
        .status();
    let ok = status.map(|s| s.success()).unwrap_or(false);
    if ok {
        println!("Rolled back {pkg} to {from}");
        0
    } else {
        eprintln!("[ERROR] Rollback failed for {pkg}");
        1
    }
}
