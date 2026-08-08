//! Native V3 `spinner` command (ADR-318) — manage ruflo verbs in Claude Code's
//! `spinnerVerbs` settings block.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/spinner.ts`. Append-only
//! (mode="append"), backup-first, ZWJ-marker-tagged; refuses to append when the
//! existing block is in `replace` mode. Consent rides on the funnel layer.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::funnel;

const DOMAIN: &str = "spinner-verbs";
const RUFLO_MARKER: &str = "\u{200d}\u{200d}\u{200d}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpinnerCommand {
    List { json: bool },
    Enable { yes: bool },
    Disable,
    Reset { yes: bool },
    Help { subcommand: Option<String> },
}

pub fn run(command: SpinnerCommand) -> u8 {
    match command {
        SpinnerCommand::List { json } => list(json),
        SpinnerCommand::Enable { yes } => enable(yes),
        SpinnerCommand::Disable => disable(),
        SpinnerCommand::Reset { yes } => reset(yes),
        SpinnerCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
    }
}

fn settings_path() -> PathBuf {
    funnel::home_dir_pub().join(".claude").join("settings.json")
}

fn read_settings() -> (Value, Option<String>) {
    let path = settings_path();
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let data = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
            (data, Some(raw))
        }
        Err(_) => (json!({}), None),
    }
}

fn backup_settings(raw: &str) -> std::io::Result<PathBuf> {
    let path = settings_path();
    let ts = funnel::now_iso_pub().replace([':', '.'], "-");
    let backup = PathBuf::from(format!("{}.bak-{ts}", path.display()));
    fs::write(&backup, raw)?;
    Ok(backup)
}

fn find_most_recent_backup() -> Option<PathBuf> {
    let path = settings_path();
    let dir = path.parent()?;
    let base = path.file_name()?.to_string_lossy().into_owned();
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with(&format!("{base}.bak-")))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();
    entries.pop()
}

fn write_settings(data: &Value) -> bool {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        if fs::create_dir_all(dir).is_err() {
            return false;
        }
    }
    let tmp = PathBuf::from(format!("{}.tmp-{}", path.display(), std::process::id()));
    let bytes = match serde_json::to_vec_pretty(data) {
        Ok(mut b) => {
            b.push(b'\n');
            b
        }
        Err(_) => return false,
    };
    let mut file = match fs::File::create(&tmp) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.write_all(&bytes).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    let _ = file.sync_all();
    drop(file);
    if fs::rename(&tmp, &path).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    true
}

fn is_ruflo_verb(v: &str) -> bool {
    v.contains(RUFLO_MARKER)
}

fn mark(v: &str) -> String {
    format!("{v}{RUFLO_MARKER}")
}

fn is_control_or_bidi(cp: u32) -> bool {
    if cp < 0x20 || cp == 0x7f {
        return true;
    }
    (0x80..=0x9f).contains(&cp)
        || (0x202a..=0x202e).contains(&cp)
        || (0x2066..=0x2069).contains(&cp)
}

/// spinner.ts:84-110 — ≤30 UTF-16 units, some whitespace-separated word ends in
/// `-ing`, no control/bidi codepoints, no http(s):// URLs.
fn is_valid_verb(v: &str) -> bool {
    let stripped = v.replace(RUFLO_MARKER, "");
    if stripped.is_empty() || stripped.encode_utf16().count() > 30 {
        return false;
    }
    let has_ing = stripped
        .split_whitespace()
        .any(|w| w.len() >= 3 && w.to_ascii_lowercase().ends_with("ing"));
    if !has_ing {
        return false;
    }
    for ch in stripped.chars() {
        if is_control_or_bidi(ch as u32) {
            return false;
        }
    }
    let lower = stripped.to_ascii_lowercase();
    !lower.contains("http://") && !lower.contains("https://")
}

/// v0 pool (spinner.ts:36-81) — 31 neutral + 6 Cognitum-tagged = 37 verbs.
fn pool() -> Vec<&'static str> {
    [
        // Memory & retrieval (7)
        "Consulting the memory graph",
        "Warming the HNSW index",
        "Recalling similar patterns",
        "Searching semantic memory",
        "Reranking with MMR",
        "Traversing knowledge graph",
        "Loading trajectory context",
        // Optimization (6)
        "Optimizing your prompt",
        "Sharpening the plan",
        "Compacting the context",
        "Distilling the trajectory",
        "Reducing token spend",
        "Compressing the working set",
        // Learning & intelligence (6)
        "Learning from the trajectory",
        "Training the router",
        "Judging past verdicts",
        "Consolidating memories",
        "Reasoning through the graph",
        "Predicting the next step",
        // Security & audit (4)
        "Auditing for CVEs",
        "Scanning dependencies",
        "Verifying signatures",
        "Guarding against injection",
        // Agents & swarm (4)
        "Spawning subagents",
        "Coordinating the swarm",
        "Reaching consensus",
        "Balancing the workload",
        // Workflow (4)
        "Routing to the best model",
        "Warming background workers",
        "Analyzing the diff",
        "Sharpening the review",
        // Cognitum-tagged (6)
        "Consulting Cognitum",
        "Checking Cognitum credits",
        "Routing via Cognitum",
        "Fetching a Cognitum tip",
        "Warming Cognitum cache",
        "Weighing Cognitum options",
    ]
    .into_iter()
    .collect()
}

fn valid_pool() -> Vec<&'static str> {
    pool().into_iter().filter(|v| is_valid_verb(v)).collect()
}

fn list(json: bool) -> u8 {
    let (data, _raw) = read_settings();
    let block = data.get("spinnerVerbs").cloned();
    let mode = block
        .as_ref()
        .and_then(|b| b.get("mode"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "(none)".into());
    let installed = block
        .as_ref()
        .and_then(|b| b.get("verbs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let installed_ruflo: Vec<String> = installed
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|v| is_ruflo_verb(v))
        .map(|v| v.replace(RUFLO_MARKER, ""))
        .collect();
    let installed_user: Vec<String> = installed
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|v| !is_ruflo_verb(v))
        .map(str::to_owned)
        .collect();
    let pool_v = valid_pool();
    let consent = if funnel::has_consent(DOMAIN) {
        "granted"
    } else {
        "not-granted"
    };

    if json {
        let summary = json!({
            "consent": consent,
            "mode": mode,
            "pool_available": pool_v,
            "installed_ruflo": installed_ruflo,
            "installed_user_authored": installed_user,
        });
        println!("{summary}");
        return 0;
    }

    println!("Consent: {consent}");
    println!("spinnerVerbs.mode in settings.json: {mode}");
    println!();
    println!("Ruflo pool ({} verbs, available):", pool_v.len());
    for v in &pool_v {
        println!("  • {v}");
    }
    println!();
    println!(
        "Currently installed ruflo verbs ({}):",
        installed_ruflo.len()
    );
    if installed_ruflo.is_empty() {
        println!("  (none — run `ruflo spinner enable --yes` to install)");
    } else {
        for v in &installed_ruflo {
            println!("  • {v}");
        }
    }
    println!();
    println!(
        "User-authored verbs ({}, untouched by ruflo):",
        installed_user.len()
    );
    for v in &installed_user {
        println!("  • {v}");
    }
    0
}

fn enable(yes: bool) -> u8 {
    let valid = valid_pool();
    if valid.is_empty() {
        eprintln!("[ERROR] Verb pool is empty after validation — refusing to write nothing.");
        return 1;
    }
    println!("The following verbs will be appended to Claude Code's spinner rotation:");
    println!();
    for v in &valid {
        println!("  • {v}");
    }
    println!();
    println!("Some verbs mention Cognitum, Ruflo's sponsor. This is opt-in and reversible via");
    println!("`ruflo spinner disable`. Claude Code's default verbs are preserved (append-only).");
    if !yes {
        println!();
        eprintln!("[WARN] Re-run with --yes to confirm.");
        return 1;
    }

    let (mut data, raw) = read_settings();
    let mut backup_path = None;
    if let Some(raw) = raw {
        match backup_settings(&raw) {
            Ok(p) => backup_path = Some(p),
            Err(_) => {
                eprintln!("[ERROR] Failed to back up settings.json — aborting before mutation.");
                return 1;
            }
        }
    }

    // spinner.ts:198-204 — refuse to append when mode=="replace" (would be inert).
    let existing_mode = data
        .get("spinnerVerbs")
        .and_then(|b| b.get("mode"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if existing_mode.as_deref() == Some("replace") {
        eprintln!(
            "[ERROR] settings.json has spinnerVerbs.mode = \"replace\" — refusing to append (would silently be inert). Either change your mode to \"append\" manually and re-run, or accept ruflo's pool as your entire set via manual edit."
        );
        return 1;
    }

    let current_verbs = data
        .get("spinnerVerbs")
        .and_then(|b| b.get("verbs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let preserved: Vec<Value> = current_verbs
        .iter()
        .filter(|v| v.as_str().map(|s| !is_ruflo_verb(s)).unwrap_or(true))
        .cloned()
        .collect();
    let new_verbs: Vec<Value> = valid.iter().map(|v| Value::String(mark(v))).collect();
    let mut combined = preserved.clone();
    combined.extend(new_verbs.clone());
    let block = json!({ "mode": "append", "verbs": combined });
    if let Some(obj) = data.as_object_mut() {
        obj.insert("spinnerVerbs".into(), block);
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert("spinnerVerbs".into(), block);
        data = Value::Object(obj);
    }
    if !write_settings(&data) {
        eprintln!("[ERROR] Failed to write settings.json");
        return 1;
    }
    funnel::record_consent(DOMAIN, true, "cli-spinner-enable");
    print_success(&format!(
        "Enabled — appended {} verbs to spinnerVerbs.",
        new_verbs.len()
    ));
    if let Some(backup) = backup_path {
        println!("Backup: {}", backup.display());
    }
    0
}

fn disable() -> u8 {
    let (mut data, raw) = read_settings();
    let verbs_len = data
        .get("spinnerVerbs")
        .and_then(|b| b.get("verbs"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if raw.is_none() || verbs_len == 0 {
        funnel::revoke_consent(DOMAIN, "cli-spinner-disable");
        println!("Nothing to disable — no spinnerVerbs block found in settings.json.");
        return 0;
    }
    let backup_path = match raw.as_ref() {
        Some(r) => match backup_settings(r) {
            Ok(p) => Some(p),
            Err(_) => {
                eprintln!("[ERROR] Failed to back up settings.json — aborting before mutation.");
                return 1;
            }
        },
        None => None,
    };
    let before = verbs_len;
    let kept: Vec<Value> = data
        .get("spinnerVerbs")
        .and_then(|b| b.get("verbs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| v.as_str().map(|s| !is_ruflo_verb(s)).unwrap_or(true))
        .collect();
    let after = kept.len();
    if let Some(obj) = data.as_object_mut() {
        if kept.is_empty() {
            obj.remove("spinnerVerbs");
        } else if let Some(block) = obj.get_mut("spinnerVerbs").and_then(Value::as_object_mut) {
            block.insert("verbs".into(), Value::Array(kept));
        }
    }
    if !write_settings(&data) {
        eprintln!("[ERROR] Failed to write settings.json");
        return 1;
    }
    funnel::revoke_consent(DOMAIN, "cli-spinner-disable");
    print_success(&format!(
        "Disabled — removed {} ruflo verbs (kept {} user-authored).",
        before - after,
        after
    ));
    if let Some(backup) = backup_path {
        println!("Backup: {}", backup.display());
    }
    0
}

fn reset(yes: bool) -> u8 {
    let Some(backup) = find_most_recent_backup() else {
        eprintln!("[WARN] No settings.json backup found — nothing to restore.");
        return 1;
    };
    if !yes {
        println!("Would restore: {}", backup.display());
        println!("Over:          {}", settings_path().display());
        println!();
        eprintln!("[WARN] Re-run with --yes to confirm.");
        return 1;
    }
    let (_data, raw) = read_settings();
    if let Some(raw) = raw {
        if backup_settings(&raw).is_err() {
            eprintln!("[ERROR] Failed to back up settings.json — aborting before restore.");
            return 1;
        }
    }
    // spinner.ts:316 restores via copyFileSync — same non-atomic copy, parity.
    if fs::copy(&backup, settings_path()).is_err() {
        eprintln!(
            "[ERROR] Failed to restore settings.json from {}",
            backup.display()
        );
        return 1;
    }
    funnel::revoke_consent(DOMAIN, "cli-spinner-reset");
    print_success(&format!("Restored settings.json from {}", backup.display()));
    0
}

fn print_success(message: &str) {
    if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        println!("\x1b[32m\x1b[1m\u{2714} {message}\x1b[0m");
    } else {
        println!("\u{2714} {message}");
    }
}

fn help(subcommand: Option<&str>) -> &'static str {
    match subcommand {
        Some("list") => "\nruflo spinner list\nShow ruflo's verb pool and which verbs are currently installed\n\nOPTIONS:\n      --json  Output as JSON [default: false]\n",
        Some("enable") => "\nruflo spinner enable\nAdd ruflo's curated verb pool to Claude Code's spinner rotation\n\nOPTIONS:\n      --yes  Skip the confirmation prompt [default: false]\n",
        Some("disable") => "\nruflo spinner disable\nRemove ruflo verbs from Claude Code's spinner rotation (user-authored verbs preserved)\n",
        Some("reset") => "\nruflo spinner reset\nRestore the most recent settings.json backup (destructive)\n\nOPTIONS:\n      --yes  Skip the confirmation prompt [default: false]\n",
        _ => "\nruflo spinner\nManage ruflo verbs in Claude Code's spinnerVerbs rotation (ADR-318)\n\nSUBCOMMANDS:\n  list     Show ruflo's verb pool and which verbs are currently installed\n  enable   Add ruflo's curated verb pool to Claude Code's spinner rotation\n  disable  Remove ruflo verbs from Claude Code's spinner rotation (user-authored verbs preserved)\n  reset    Restore the most recent settings.json backup (destructive)\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_detection_and_marking() {
        assert!(is_ruflo_verb(&mark("Thinking")));
        assert!(!is_ruflo_verb("Thinking"));
        assert_eq!(mark("x").replace(RUFLO_MARKER, ""), "x");
    }

    #[test]
    fn validation_requires_ing_word_and_caps_length() {
        assert!(is_valid_verb("Optimizing your prompt")); // multi-word, word ends -ing
        assert!(!is_valid_verb("Hello world")); // no -ing word
        assert!(is_valid_verb("Thinking"));
        // length cap is 30 UTF-16 units AND requires an -ing word.
        assert!(!is_valid_verb(&"x".repeat(31))); // too long, no -ing
        assert!(!is_valid_verb(&"a".repeat(30))); // 30 units but no -ing word -> invalid
                                                  // exactly 30 units with a whitespace -ing word -> valid; 31 -> too long
        let exactly_30 = format!("{} Thinking", "x".repeat(21)); // 21 + 1 + 8 = 30
        let over_30 = format!("{} Thinking", "x".repeat(22)); // 22 + 1 + 8 = 31
        assert_eq!(exactly_30.encode_utf16().count(), 30);
        assert!(is_valid_verb(&exactly_30));
        assert!(!is_valid_verb(&over_30));
        assert!(!is_valid_verb("see http://x.example"));
        assert!(!is_valid_verb("bad\u{202e}bidi"));
    }

    #[test]
    fn validation_counts_utf16_units() {
        // 15 non-BMP emoji each = 2 UTF-16 units = 30 units, but no -ing word -> invalid anyway.
        // Build a 30-unit string with an -ing word: "🤔🤔🤔🤔🤔🤔🤔🤔🤔🤔🤔🤔🤔Thinking" — count check.
        let s = format!("{}Thinking", "🤔".repeat(7)); // 7*2 + 8 = 22 units, ok
        assert!(is_valid_verb(&s));
        let too_long = format!("{}Thinking", "🤔".repeat(12)); // 24 + 8 = 32 units > 30
        assert!(!is_valid_verb(&too_long));
    }

    #[test]
    fn pool_entries_all_validate_and_count_is_37() {
        assert_eq!(pool().len(), 37);
        for v in pool() {
            assert!(is_valid_verb(v), "pool verb failed validation: {v}");
        }
        assert_eq!(valid_pool().len(), 37);
    }
}
