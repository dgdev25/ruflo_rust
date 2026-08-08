//! End-to-end `announcements` command tests through both native binaries (ADR-319).
//!
//! Source: v3/@claude-flow/cli/src/commands/announcements.ts. HOME is isolated so
//! ~/.claude/settings.json (companyAnnouncements) and ~/.ruflo (consent) never
//! touch the real user state.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

fn isolated() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let home_path = home.path().to_path_buf();
    (home, home_path)
}

#[test]
fn list_default_shows_pool_and_consent_state() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        let out = run(binary, &home_path, &["announcements", "list"]);
        assert_success(&out);
        let s = stdout(&out);
        assert!(s.contains("Consent: not-granted"));
        assert!(s.contains("Ruflo pool (12 announcements, available):"));
        assert!(s.contains("Cognitum"));
        assert!(s.contains("(none — run"));

        let js = run(binary, &home_path, &["announcements", "list", "--json"]);
        assert_success(&js);
        let v: Value = serde_json::from_str(stdout(&js).trim()).unwrap();
        assert_eq!(v["consent"], "not-granted");
        assert_eq!(v["pool_available"].as_array().unwrap().len(), 12);
    }
}

#[test]
fn enable_requires_yes_and_is_idempotent_and_preserves_user_entries() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();

        // Seed a user-authored announcement + unrelated key.
        std::fs::create_dir_all(home_path.join(".claude")).unwrap();
        std::fs::write(
            settings(&home_path),
            serde_json::json!({
                "companyAnnouncements": ["My own note"],
                "shouldKeep": true,
            })
            .to_string(),
        )
        .unwrap();

        // Without --yes: previews, writes nothing.
        let prompt = run(binary, &home_path, &["announcements", "enable"]);
        assert_eq!(prompt.status.code(), Some(1));
        assert!(stdout(&prompt).contains("appended to Claude Code"));
        assert!(stderr(&prompt).contains("Re-run with --yes"));
        let before = read_settings(&home_path);
        assert_eq!(before["companyAnnouncements"].as_array().unwrap().len(), 1);

        // With --yes: appends ruflo entries, keeps user note + unrelated key, backs up.
        let enable = run(binary, &home_path, &["announcements", "enable", "--yes"]);
        assert_success(&enable);
        assert!(stdout(&enable).contains("Enabled — appended 12 announcements"));
        assert!(stdout(&enable).contains("Backup:"));
        let after = read_settings(&home_path);
        let arr = after["companyAnnouncements"].as_array().unwrap();
        assert_eq!(arr.len(), 13); // 1 user + 12 ruflo
        assert_eq!(arr[0].as_str(), Some("My own note"));
        assert!(arr[1].as_str().unwrap().contains('\u{200d}')); // ZWJ marker
        assert_eq!(after["shouldKeep"], true);
        assert!(home_path.join(".claude").read_dir().unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("settings.json.bak-")
        }));

        // Re-enable is idempotent: ruflo entries replaced, user note still first.
        let again = run(binary, &home_path, &["announcements", "enable", "--yes"]);
        assert_success(&again);
        let after2 = read_settings(&home_path);
        let arr2 = after2["companyAnnouncements"].as_array().unwrap();
        assert_eq!(arr2.len(), 13);
        assert_eq!(arr2[0].as_str(), Some("My own note"));
    }
}

#[test]
fn disable_removes_ruflo_keeps_user_and_drops_empty_key() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        run(binary, &home_path, &["announcements", "enable", "--yes"]);
        // add a user entry manually
        let mut s = read_settings(&home_path);
        s["companyAnnouncements"]
            .as_array_mut()
            .unwrap()
            .insert(0, Value::String("user note".into()));
        write_settings(&home_path, &s);

        let disable = run(binary, &home_path, &["announcements", "disable"]);
        assert_success(&disable);
        assert!(stdout(&disable).contains("removed 12 ruflo announcements (kept 1 user-authored"));
        let after = read_settings(&home_path);
        let arr = after["companyAnnouncements"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].as_str(), Some("user note"));

        // Disable again: now only the user note remains; disabling removes nothing.
        let again = run(binary, &home_path, &["announcements", "disable"]);
        assert_success(&again);

        // When the array would be empty, the key is dropped entirely.
        let mut s2 = read_settings(&home_path);
        s2["companyAnnouncements"] = Value::Array(vec![]);
        write_settings(&home_path, &s2);
        // seed a ruflo entry to remove so disable has work
        let mut s3 = read_settings(&home_path);
        s3["companyAnnouncements"] =
            Value::Array(vec![Value::String("x\u{200d}\u{200d}\u{200d}".to_string())]);
        write_settings(&home_path, &s3);
        let drop_test = run(binary, &home_path, &["announcements", "disable"]);
        assert_success(&drop_test);
        let final_s = read_settings(&home_path);
        assert!(final_s.get("companyAnnouncements").is_none());
    }
}

#[test]
fn reset_restores_most_recent_backup_with_yes() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        std::fs::create_dir_all(home_path.join(".claude")).unwrap();
        std::fs::write(
            settings(&home_path),
            "{\"companyAnnouncements\":[\"original\"]}",
        )
        .unwrap();

        // No backup yet -> reset warns.
        let none = run(binary, &home_path, &["announcements", "reset"]);
        assert_eq!(none.status.code(), Some(1));
        assert!(stderr(&none).contains("No settings.json backup found"));

        // enable creates a backup, mutates settings.
        run(binary, &home_path, &["announcements", "enable", "--yes"]);
        assert!(
            read_settings(&home_path)["companyAnnouncements"]
                .as_array()
                .unwrap()
                .len()
                > 1
        );

        // reset without --yes previews.
        let preview = run(binary, &home_path, &["announcements", "reset"]);
        assert_eq!(preview.status.code(), Some(1));
        assert!(stdout(&preview).contains("Would restore:"));

        // reset --yes restores the backup (original single-entry file).
        let reset = run(binary, &home_path, &["announcements", "reset", "--yes"]);
        assert_success(&reset);
        assert!(stdout(&reset).contains("Restored settings.json"));
        let restored = read_settings(&home_path);
        let arr = restored["companyAnnouncements"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].as_str(), Some("original"));
    }
}

#[test]
fn unknown_subcommand_errors() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        let bad = run(binary, &home_path, &["announcements", "frobnicate"]);
        assert_ne!(bad.status.code(), Some(0));
        assert!(stderr(&bad).contains("Unknown subcommand 'frobnicate'"));
    }
}

fn settings(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

fn read_settings(home: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(settings(home)).unwrap()).unwrap()
}

fn write_settings(home: &Path, v: &Value) {
    std::fs::write(settings(home), serde_json::to_string(v).unwrap()).unwrap();
}

fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn run(binary: &str, home: &Path, args: &[&str]) -> Output {
    let _g = HOME_LOCK.lock().unwrap();
    let project = tempfile::tempdir().unwrap();
    Command::new(executable(binary))
        .current_dir(project.path())
        .args(args)
        .env("HOME", home)
        .env("NO_COLOR", "1")
        .env_remove("RUFLO_STATE_DIR")
        .output()
        .unwrap()
}

static HOME_LOCK: Mutex<()> = Mutex::new(());

fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|name| name == binary) {
        let status = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(status.success(), "failed to build {binary}");
        built.push(binary.to_string());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
