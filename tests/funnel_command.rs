//! End-to-end `funnel` command tests through both native binaries (ADR-301/317).
//!
//! Source: v3/@claude-flow/cli/src/commands/funnel.ts. RUFLO_STATE_DIR isolates
//! ~/.ruflo (state, disclosure, payout, funnel-id, statusline-promo).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

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

#[test]
fn status_default_enabled_package_default_and_json_shape() {
    for binary in ["ruflo", "claude-flow"] {
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let out = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "status"],
        );
        assert_success(&out);
        let s = stdout(&out);
        assert!(s.contains("Funnel: enabled (decided by: package-default)"));
        assert!(s.contains("Disclosure: never_seen"));
        assert!(s.contains("Consents: none recorded"));

        let js = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "status", "--json"],
        );
        assert_success(&js);
        let v: Value = serde_json::from_str(stdout(&js).trim()).unwrap();
        assert_eq!(v["enabled"], true);
        assert_eq!(v["decidedBy"], "package-default");
        assert_eq!(v["disclosure"], "never_seen");
    }
}

#[test]
fn disable_then_enable_and_accept_backdates_disclosure() {
    for binary in ["ruflo", "claude-flow"] {
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();

        let off = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "disable"],
        );
        assert_success(&off);
        assert!(stdout(&off).contains("Funnel disabled."));
        let cfg: Value = serde_json::from_str(&read(state.path(), "funnel.json")).unwrap();
        assert_eq!(cfg["enabled"], false);
        assert!(!state.path().join("funnel-id.json").exists());

        // disabled state -> status reports user-config
        let st = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "status"],
        );
        assert!(stdout(&st).contains("disabled (decided by: user-config)"));

        let on = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "enable"],
        );
        assert_success(&on);
        assert!(stdout(&on).contains("Funnel enabled."));

        // accept backdates firstShownAt past the grace so promo is eligible
        let acc = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "accept"],
        );
        assert_success(&acc);
        assert!(stdout(&acc).contains("Disclosure accepted"));
        assert!(stdout(&acc).contains("Promo rotation eligible: true"));
    }
}

#[test]
fn accept_refused_when_disabled() {
    for binary in ["ruflo", "claude-flow"] {
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "disable"],
        );
        let acc = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "accept"],
        );
        assert_eq!(acc.status.code(), Some(1));
        assert!(stderr(&acc).contains("Run 'ruflo funnel enable'"));
    }
}

#[test]
fn open_rejects_absent_promo_and_non_allowlisted_url() {
    for binary in ["ruflo", "claude-flow"] {
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();

        // no promo memo -> warn
        let none = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "open"],
        );
        assert_eq!(none.status.code(), Some(1));
        assert!(stderr(&none).contains("No promo has been shown yet"));

        // non-allowlisted https URL -> refused. readCurrentPromo reads
        // ~/.ruflo/statusline-promo.json (os.homedir, NOT funnelStateDir).
        std::fs::create_dir_all(home.path().join(".ruflo")).unwrap();
        std::fs::write(
            home.path().join(".ruflo").join("statusline-promo.json"),
            r#"{"promo":{"text":"x","url":"https://evil.example/x","kind":"tip"}}"#,
        )
        .unwrap();
        let bad = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "open"],
        );
        assert_eq!(bad.status.code(), Some(1));
        assert!(stderr(&bad).contains("not on the allowlist"));
    }
}

#[test]
fn enroll_requires_enabled_and_accepted_disclosure_then_earnings_not_enrolled() {
    for binary in ["ruflo", "claude-flow"] {
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();

        // disabled -> enroll warns
        run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "disable"],
        );
        let e = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "enroll"],
        );
        assert_eq!(e.status.code(), Some(1));
        assert!(stderr(&e).contains("Enable it (ruflo funnel enable)"));

        run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "enable"],
        );
        // enable sets disclosure=disclosed_enabled, so enroll proceeds without a
        // separate `accept` (accept only backdates the promo-rotation grace).
        let e3 = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "enroll"],
        );
        assert_success(&e3);
        assert!(stdout(&e3).contains("Consent recorded for rev-share-payout."));
        assert!(stdout(&e3).contains(ENROLL_URL));

        // Phase 0: no backend -> not enrolled, not earning
        let earn = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "earnings"],
        );
        assert_success(&earn);
        assert!(stdout(&earn).contains("Enrolled: no"));
        assert!(stdout(&earn).contains("Not enrolled."));

        // unenroll clears consent
        let un = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "unenroll"],
        );
        assert_success(&un);
        assert!(
            stdout(&un).contains("Unenrolled locally.")
                || stdout(&un).contains("Nothing to unenroll")
        );
    }
}

#[test]
fn id_without_telemetry_shows_none() {
    for binary in ["ruflo", "claude-flow"] {
        let home = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let id = run(
            binary,
            cwd.path(),
            home.path(),
            state.path(),
            &["funnel", "id"],
        );
        assert_success(&id);
        assert!(stdout(&id).contains("No funnel ID"));
    }
}

const ENROLL_URL: &str = "https://funnel.ruv.io/enroll";

fn read(state: &Path, name: &str) -> String {
    std::fs::read_to_string(state.join(name)).unwrap()
}
fn assert_success(o: &Output) {
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(o));
}
fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}
fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|n| n == binary) {
        let s = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(s.success());
        built.push(binary.into());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
