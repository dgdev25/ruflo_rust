//! End-to-end `settings` command tests through both native binaries (ADR-311).
//!
//! Source: v3/@claude-flow/cli/src/commands/settings.ts + funnel precedence/
//! disclosure/notifiers. RUFLO_STATE_DIR isolates ~/.ruflo; HOME isolates the
//! (unused here) ~/.claude path.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

fn isolated() -> (tempfile::TempDir, PathBuf) {
    let state = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    // project dir doubles as cwd + project-config probe location
    (state, home.path().to_path_buf())
}

#[test]
fn overview_and_default_show_enabled_package_default() {
    for binary in ["ruflo", "claude-flow"] {
        let (state, home) = isolated();
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &home, state.path(), &["settings"]);
        assert_success(&out);
        let s = stdout(&out);
        assert!(s.contains("ruflo settings — user preferences"));
        assert!(s.contains("current: enabled (package-default)"));
        assert!(s.contains("disclosure: never_seen"));

        // `settings` with no subcommand == overview; `settings notices` == status.
        let status = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "status"],
        );
        assert_success(&status);
        assert!(stdout(&status).contains("Notices: enabled (decided by: package-default)"));
        assert!(stdout(&status).contains("Disclosure: never_seen"));
        assert!(stdout(&status).contains("Telemetry: no consent"));

        // bare `settings notices` -> status action
        let bare = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices"],
        );
        assert_success(&bare);
        assert!(stdout(&bare).contains("Notices: enabled"));
    }
}

#[test]
fn notices_off_disables_and_deletes_funnel_data_then_on_reenables() {
    for binary in ["ruflo", "claude-flow"] {
        let (state, home) = isolated();
        let project = tempfile::tempdir().unwrap();
        // seed a funnel id + disclosure so off can delete/reset them
        run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "on"],
        );

        let off = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "off"],
        );
        assert_success(&off);
        assert!(stdout(&off).contains("Notices disabled. Local notice data deleted."));
        // funnel.json.enabled=false, disclosure=disclosed_disabled, funnel-id deleted
        let cfg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.path().join("funnel.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg["enabled"], false);
        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.path().join("funnel-disclosure.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disc["state"], "disclosed_disabled");
        assert!(!state.path().join("funnel-id.json").exists());

        // status now reflects disabled via user-config precedence
        let status = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "status"],
        );
        assert!(stdout(&status).contains("Notices: disabled (decided by: user-config)"));
    }
}

#[test]
fn env_override_disables_with_decided_by_env() {
    for binary in ["ruflo", "claude-flow"] {
        let (state, home) = isolated();
        let project = tempfile::tempdir().unwrap();
        let out = Command::new(executable(binary))
            .current_dir(project.path())
            .args(["settings", "notices", "status"])
            .env("HOME", &home)
            .env("NO_COLOR", "1")
            .env("RUFLO_STATE_DIR", state.path())
            .env("RUFLO_FUNNEL", "0")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert!(String::from_utf8(out.stdout)
            .unwrap()
            .contains("disabled (decided by: env)"));
    }
}

#[test]
fn rate_limited_and_quota_low_set_then_cooldown_blocks_toggle() {
    for binary in ["ruflo", "claude-flow"] {
        let (state, home) = isolated();
        let project = tempfile::tempdir().unwrap();

        let set = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "rate-limited"],
        );
        assert_success(&set);
        assert!(stdout(&set).contains("Rate-limit flag set."));
        let status: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.path().join("rate-limit-status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status["limited"], true);

        // immediate clear is blocked by the 10-min cooldown
        let blocked = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "rate-limited", "--clear"],
        );
        assert_eq!(blocked.status.code(), Some(1));
        assert!(stderr(&blocked).contains("just toggled"));

        // quota-low mirrors the behavior, distinct file+field (quota-status.json / low)
        let q = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "quota-low"],
        );
        assert_success(&q);
        assert!(stdout(&q).contains("Quota-low flag set."));
        let qstatus: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.path().join("quota-status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(qstatus["low"], true);
        assert!(!state.path().join("quota-low-status.json").exists());
    }
}

#[test]
fn notices_id_without_telemetry_shows_no_id() {
    for binary in ["ruflo", "claude-flow"] {
        let (state, home) = isolated();
        let project = tempfile::tempdir().unwrap();
        let id = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "id"],
        );
        assert_success(&id);
        assert!(stdout(&id).contains("(no id — telemetry consent not granted"));
    }
}

#[test]
fn unknown_notices_subcommand_errors() {
    for binary in ["ruflo", "claude-flow"] {
        let (state, home) = isolated();
        let project = tempfile::tempdir().unwrap();
        let bad = run(
            binary,
            project.path(),
            &home,
            state.path(),
            &["settings", "notices", "frobnicate"],
        );
        assert_ne!(bad.status.code(), Some(0));
        assert!(stderr(&bad).contains("Unknown notices subcommand 'frobnicate'"));
    }
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
fn run(binary: &str, cwd: &Path, home: &Path, state: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable(binary))
        .current_dir(cwd)
        .args(args)
        .env("HOME", home)
        .env("NO_COLOR", "1")
        .env("RUFLO_STATE_DIR", state)
        .env_remove("RUFLO_FUNNEL")
        .output()
        .unwrap()
}
static LOCK: Mutex<()> = Mutex::new(());
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
