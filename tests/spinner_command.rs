//! End-to-end `spinner` command tests through both native binaries (ADR-318).
//!
//! Source: v3/@claude-flow/cli/src/commands/spinner.ts. HOME isolated so
//! ~/.claude/settings.json (spinnerVerbs) and ~/.ruflo (consent) are sandboxed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

fn isolated() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let p = home.path().to_path_buf();
    (home, p)
}

#[test]
fn list_default_shows_pool_mode_none() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        let out = run(binary, &home_path, &["spinner", "list"]);
        assert_success(&out);
        let s = stdout(&out);
        assert!(s.contains("Consent: not-granted"));
        assert!(s.contains("spinnerVerbs.mode in settings.json: (none)"));
        assert!(s.contains("Ruflo pool (37 verbs, available):"));

        let js = run(binary, &home_path, &["spinner", "list", "--json"]);
        assert_success(&js);
        let v: Value = serde_json::from_str(stdout(&js).trim()).unwrap();
        assert_eq!(v["mode"], "(none)");
        assert_eq!(v["pool_available"].as_array().unwrap().len(), 37);
    }
}

#[test]
fn enable_requires_yes_idempotent_preserves_user_and_sets_append_mode() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        std::fs::create_dir_all(home_path.join(".claude")).unwrap();
        std::fs::write(
            settings(&home_path),
            serde_json::json!({
                "spinnerVerbs": {"mode": "append", "verbs": ["UserThinking"]},
                "keepMe": 7,
            })
            .to_string(),
        )
        .unwrap();

        // Without --yes: preview only, no write.
        let prompt = run(binary, &home_path, &["spinner", "enable"]);
        assert_eq!(prompt.status.code(), Some(1));
        assert!(stderr(&prompt).contains("Re-run with --yes"));
        assert_eq!(
            read_settings(&home_path)["spinnerVerbs"]["verbs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // With --yes: appends 37, keeps user verb first, mode=append, unrelated key kept.
        let enable = run(binary, &home_path, &["spinner", "enable", "--yes"]);
        assert_success(&enable);
        assert!(stdout(&enable).contains("Enabled — appended 37 verbs to spinnerVerbs."));
        let after = read_settings(&home_path);
        let block = &after["spinnerVerbs"];
        assert_eq!(block["mode"], "append");
        let verbs = block["verbs"].as_array().unwrap();
        assert_eq!(verbs.len(), 38); // 1 user + 37 ruflo
        assert_eq!(verbs[0].as_str(), Some("UserThinking"));
        assert!(verbs[1].as_str().unwrap().contains('\u{200d}'));
        assert_eq!(after["keepMe"], 7);

        // Idempotent re-enable.
        let again = run(binary, &home_path, &["spinner", "enable", "--yes"]);
        assert_success(&again);
        let after2 = read_settings(&home_path);
        assert_eq!(
            after2["spinnerVerbs"]["verbs"].as_array().unwrap().len(),
            38
        );
    }
}

#[test]
fn enable_refuses_when_mode_is_replace() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        std::fs::create_dir_all(home_path.join(".claude")).unwrap();
        std::fs::write(
            settings(&home_path),
            serde_json::json!({"spinnerVerbs": {"mode": "replace", "verbs": ["OnlyThinking"]}})
                .to_string(),
        )
        .unwrap();
        let enable = run(binary, &home_path, &["spinner", "enable", "--yes"]);
        assert_eq!(enable.status.code(), Some(1));
        assert!(stderr(&enable).contains("mode = \"replace\""));
        // settings untouched
        assert_eq!(
            read_settings(&home_path)["spinnerVerbs"]["verbs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn disable_removes_ruflo_keeps_user_and_drops_empty_block() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        run(binary, &home_path, &["spinner", "enable", "--yes"]);
        // add a user verb
        let mut s = read_settings(&home_path);
        s["spinnerVerbs"]["verbs"]
            .as_array_mut()
            .unwrap()
            .insert(0, Value::String("UserThinking".into()));
        write_settings(&home_path, &s);

        let disable = run(binary, &home_path, &["spinner", "disable"]);
        assert_success(&disable);
        assert!(stdout(&disable).contains("removed 37 ruflo verbs (kept 1 user-authored"));
        let after = read_settings(&home_path);
        let verbs = after["spinnerVerbs"]["verbs"].as_array().unwrap();
        assert_eq!(verbs.len(), 1);
        assert_eq!(verbs[0].as_str(), Some("UserThinking"));

        // When only ruflo verbs remain, disable drops the whole block.
        let mut s2 = read_settings(&home_path);
        s2["spinnerVerbs"] =
            json!({"mode":"append","verbs":[format!("x\u{200d}\u{200d}\u{200d}")]});
        write_settings(&home_path, &s2);
        let drop_test = run(binary, &home_path, &["spinner", "disable"]);
        assert_success(&drop_test);
        assert!(read_settings(&home_path).get("spinnerVerbs").is_none());
    }
}

#[test]
fn reset_restores_backup_with_yes() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        std::fs::create_dir_all(home_path.join(".claude")).unwrap();
        std::fs::write(
            settings(&home_path),
            "{\"spinnerVerbs\":{\"verbs\":[\"orig\"]}}",
        )
        .unwrap();

        let none = run(binary, &home_path, &["spinner", "reset"]);
        assert_eq!(none.status.code(), Some(1));
        assert!(stderr(&none).contains("No settings.json backup found"));

        run(binary, &home_path, &["spinner", "enable", "--yes"]);

        let preview = run(binary, &home_path, &["spinner", "reset"]);
        assert_eq!(preview.status.code(), Some(1));
        assert!(stdout(&preview).contains("Would restore:"));

        let reset = run(binary, &home_path, &["spinner", "reset", "--yes"]);
        assert_success(&reset);
        assert!(stdout(&reset).contains("Restored settings.json"));
        let restored = read_settings(&home_path);
        assert_eq!(
            restored["spinnerVerbs"]["verbs"].as_array().unwrap()[0].as_str(),
            Some("orig")
        );
    }
}

#[test]
fn unknown_subcommand_errors() {
    for binary in ["ruflo", "claude-flow"] {
        let (_home, home_path) = isolated();
        let bad = run(binary, &home_path, &["spinner", "frobnicate"]);
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

use serde_json::json;
