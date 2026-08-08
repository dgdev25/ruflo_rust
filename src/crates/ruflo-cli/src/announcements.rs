//! Native V3 `announcements` command (ADR-319) — manage ruflo entries in Claude
//! Code's `companyAnnouncements` settings array.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/announcements.ts`.
//! Append-only, backup-first, ZWJ-marker-tagged for clean removal; user-authored
//! entries are always preserved. Consent rides on the funnel layer (ADR-302).

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::funnel;

const DOMAIN: &str = "company-announcements";
/// Three zero-width joiners — invisible, unlikely in user text (announcements.ts:24).
const RUFLO_MARKER: &str = "\u{200d}\u{200d}\u{200d}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnouncementsCommand {
    List { json: bool },
    Enable { yes: bool },
    Disable,
    Reset { yes: bool },
    Help { subcommand: Option<String> },
}

pub fn run(command: AnnouncementsCommand) -> u8 {
    match command {
        AnnouncementsCommand::List { json } => list(json),
        AnnouncementsCommand::Enable { yes } => enable(yes),
        AnnouncementsCommand::Disable => disable(),
        AnnouncementsCommand::Reset { yes } => reset(yes),
        AnnouncementsCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
    }
}

fn settings_path() -> PathBuf {
    crate::funnel::home_dir_pub()
        .join(".claude")
        .join("settings.json")
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

/// Backup the current raw settings before a mutation. announcements.ts throws
/// on backup failure, so the caller treats `Err` as fatal (abort, do not mutate).
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
                .map(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with(&format!("{base}.bak-"))
                })
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

fn is_ruflo_entry(a: &str) -> bool {
    a.contains(RUFLO_MARKER)
}

fn mark(a: &str) -> String {
    format!("{a}{RUFLO_MARKER}")
}

/// announcements.ts:57-64 — control / bidi-override codepoints (per-codepoint,
/// not byte-regex, so emoji survive validation).
fn is_control_or_bidi(cp: u32) -> bool {
    if cp < 0x20 {
        return true;
    }
    if cp == 0x7f {
        return true;
    }
    (0x80..=0x9f).contains(&cp) // C1
        || (0x202a..=0x202e).contains(&cp) // bidi override
        || (0x2066..=0x2069).contains(&cp) // bidi isolate
}

fn is_valid_announcement(a: &str) -> bool {
    let stripped = a.replace(RUFLO_MARKER, "");
    if stripped.is_empty() {
        return false;
    }
    // announcements.ts:68 uses JS `.length` (UTF-16 code units), not scalar-value
    // count — so non-BMP emoji count as 2. Match that exactly.
    if stripped.encode_utf16().count() > 140 {
        return false;
    }
    for ch in stripped.chars() {
        if is_control_or_bidi(ch as u32) {
            return false;
        }
    }
    // announcements.ts:72 — reject http(s):// URLs.
    let lower = stripped.to_ascii_lowercase();
    !lower.contains("http://") && !lower.contains("https://")
}

/// Curated v0 pool (announcements.ts:30-45). ~75% neutral / ~25% Cognitum —
/// the ratio the source ships (9 neutral, 3 Cognitum).
fn pool() -> Vec<&'static str> {
    [
        "🧠 Ruflo intelligence is learning from your work — try `ruflo intelligence stats` to see progress.",
        "📊 Statusline promo row is on — `ruflo funnel status` to manage.",
        "🔧 12 background workers help maintain your codebase — `ruflo daemon start` to enable.",
        "🔍 Semantic memory search across projects — try `ruflo memory search --query \"auth\"`.",
        "🩺 Run `ruflo doctor --fix` if something feels off.",
        "✨ 37 spinner verbs available — `ruflo spinner list` to see the pool.",
        "🛡 Security scanner is available — `ruflo security scan --depth deep`.",
        "💾 Nightly memory backups keep your intelligence safe — `ruflo daemon status`.",
        "🎯 3-tier model routing keeps spend down — `ruflo cost report` for details.",
        "📣 Ruflo is sponsored by Cognitum — visit cognitum.one to learn more.",
        "💳 Check your Cognitum credits: `ruflo proxy status`.",
        "⚡ Cognitum handles overflow routing when your model hits limits.",
    ]
    .into_iter()
    .collect()
}

fn valid_pool() -> Vec<&'static str> {
    pool()
        .into_iter()
        .filter(|a| is_valid_announcement(a))
        .collect()
}

fn list(json: bool) -> u8 {
    let (data, _raw) = read_settings();
    let installed = data
        .get("companyAnnouncements")
        .cloned()
        .unwrap_or(json!([]));
    let installed_arr = installed.as_array().cloned().unwrap_or_default();
    let installed_ruflo: Vec<String> = installed_arr
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|a| is_ruflo_entry(a))
        .map(|a| a.replace(RUFLO_MARKER, ""))
        .collect();
    let installed_user: Vec<String> = installed_arr
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|a| !is_ruflo_entry(a))
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
            "pool_available": pool_v,
            "installed_ruflo": installed_ruflo,
            "installed_user_authored": installed_user,
        });
        println!("{summary}");
        return 0;
    }

    println!("Consent: {consent}");
    println!();
    println!("Ruflo pool ({} announcements, available):", pool_v.len());
    for a in &pool_v {
        println!("  • {a}");
    }
    println!();
    println!(
        "Currently installed ruflo announcements ({}):",
        installed_ruflo.len()
    );
    if installed_ruflo.is_empty() {
        println!("  (none — run `ruflo announcements enable --yes` to install)");
    } else {
        for a in &installed_ruflo {
            println!("  • {a}");
        }
    }
    println!();
    println!(
        "User-authored announcements ({}, untouched by ruflo):",
        installed_user.len()
    );
    for a in &installed_user {
        println!("  • {a}");
    }
    0
}

fn enable(yes: bool) -> u8 {
    let valid = valid_pool();
    if valid.is_empty() {
        eprintln!(
            "[ERROR] Announcement pool is empty after validation — refusing to write nothing."
        );
        return 1;
    }
    println!("The following announcements will be appended to Claude Code's startup rotation:");
    println!();
    for a in &valid {
        println!("  • {a}");
    }
    println!();
    println!("Some announcements mention Cognitum, Ruflo's sponsor. Opt-in and reversible via");
    println!(
        "`ruflo announcements disable`. Existing announcements from your config are preserved."
    );
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
    let current = data
        .get("companyAnnouncements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // strip prior ruflo entries so re-running enable is idempotent
    let preserved: Vec<Value> = current
        .iter()
        .filter(|a| a.as_str().map(|s| !is_ruflo_entry(s)).unwrap_or(true))
        .cloned()
        .collect();
    let new_entries: Vec<Value> = valid.iter().map(|a| Value::String(mark(a))).collect();
    let mut combined = preserved.clone();
    combined.extend(new_entries.clone());
    if let Some(obj) = data.as_object_mut() {
        obj.insert("companyAnnouncements".into(), Value::Array(combined));
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert("companyAnnouncements".into(), Value::Array(combined));
        data = Value::Object(obj);
    }
    if !write_settings(&data) {
        eprintln!("[ERROR] Failed to write settings.json");
        return 1;
    }
    funnel::record_consent(DOMAIN, true, "cli-announcements-enable");
    print_success(&format!(
        "Enabled — appended {} announcements to companyAnnouncements.",
        new_entries.len()
    ));
    if let Some(backup) = backup_path {
        println!("Backup: {}", backup.display());
    }
    0
}

fn disable() -> u8 {
    let (mut data, raw) = read_settings();
    if raw.is_none() {
        funnel::revoke_consent(DOMAIN, "cli-announcements-disable");
        println!("Nothing to disable — no companyAnnouncements found in settings.json.");
        return 0;
    }
    let current_len = data
        .get("companyAnnouncements")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if current_len == 0 {
        funnel::revoke_consent(DOMAIN, "cli-announcements-disable");
        println!("Nothing to disable — no companyAnnouncements found in settings.json.");
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
    let kept: Vec<Value> = data
        .get("companyAnnouncements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.as_str().map(|s| !is_ruflo_entry(s)).unwrap_or(true))
        .collect();
    let after = kept.len();
    let removed = current_len - after;
    if let Some(obj) = data.as_object_mut() {
        if kept.is_empty() {
            obj.remove("companyAnnouncements");
        } else {
            obj.insert("companyAnnouncements".into(), Value::Array(kept));
        }
    }
    if !write_settings(&data) {
        eprintln!("[ERROR] Failed to write settings.json");
        return 1;
    }
    funnel::revoke_consent(DOMAIN, "cli-announcements-disable");
    print_success(&format!(
        "Disabled — removed {removed} ruflo announcements (kept {after} user-authored)."
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
    // announcements.ts:256 restores via copyFileSync (same non-atomic, no-newline
    // guarantee) — parity preserved here intentionally.
    if fs::copy(&backup, settings_path()).is_err() {
        eprintln!(
            "[ERROR] Failed to restore settings.json from {}",
            backup.display()
        );
        return 1;
    }
    funnel::revoke_consent(DOMAIN, "cli-announcements-reset");
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
        Some("list") => "\nruflo announcements list\nShow ruflo's announcement pool and what's currently installed\n\nOPTIONS:\n      --json  Output as JSON [default: false]\n",
        Some("enable") => "\nruflo announcements enable\nAdd ruflo's curated announcements to Claude Code's startup rotation\n\nOPTIONS:\n      --yes  Skip the confirmation prompt [default: false]\n",
        Some("disable") => "\nruflo announcements disable\nRemove ruflo announcements (user-authored ones preserved)\n",
        Some("reset") => "\nruflo announcements reset\nRestore the most recent settings.json backup (destructive)\n\nOPTIONS:\n      --yes  Skip the confirmation prompt [default: false]\n",
        _ => "\nruflo announcements\nManage ruflo entries in Claude Code's companyAnnouncements startup rotation (ADR-319)\n\nSUBCOMMANDS:\n  list     Show ruflo's announcement pool and what's currently installed\n  enable   Add ruflo's curated announcements to Claude Code's startup rotation\n  disable  Remove ruflo announcements (user-authored ones preserved)\n  reset    Restore the most recent settings.json backup (destructive)\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_detection_and_marking() {
        assert!(!is_ruflo_entry("plain"));
        assert!(is_ruflo_entry(&mark("plain")));
        assert_eq!(mark("x").replace(RUFLO_MARKER, ""), "x");
    }

    #[test]
    fn validation_rejects_urls_controls_and_oversize() {
        assert!(is_valid_announcement("🧠 short tip"));
        assert!(!is_valid_announcement("see http://evil.example"));
        assert!(!is_valid_announcement("see https://evil.example"));
        assert!(!is_valid_announcement(&"x".repeat(141)));
        assert!(is_valid_announcement(&"x".repeat(140)));
        assert!(!is_valid_announcement("bad\u{0007}bell"));
        assert!(!is_valid_announcement("bad\u{202e}bidi"));
    }

    #[test]
    fn pool_entries_all_validate() {
        // The curated pool must pass its own validator (emoji-safe check).
        let v = valid_pool();
        assert!(!v.is_empty());
        for a in pool() {
            assert!(
                is_valid_announcement(a),
                "pool entry failed validation: {a}"
            );
        }
    }

    #[test]
    fn validation_counts_utf16_units_like_javascript() {
        // One non-BMP emoji = 2 UTF-16 units. JS `.length` sees 71 such emoji as
        // 142 (>140) and rejects; a scalar-value count (71) would wrongly accept.
        let emoji = "😀".repeat(71);
        assert!(!is_valid_announcement(&emoji));
        // 70 emoji = 140 UTF-16 units, exactly at the cap, accepted.
        assert!(is_valid_announcement(&"😀".repeat(70)));
    }
}
