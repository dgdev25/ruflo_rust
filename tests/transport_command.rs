use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    environment: BTreeMap<String, String>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    argv: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
}

#[test]
fn both_native_aliases_replay_the_source_transport_contract() {
    let fixture: Fixture = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/cli/transport/v3.json").unwrap(),
    )
    .unwrap();
    for binary in ["ruflo", "claude-flow"] {
        for case in &fixture.cases {
            let project = tempfile::tempdir().unwrap();
            let output = Command::new(executable(binary))
                .current_dir(project.path())
                .args(&case.argv)
                .envs(&fixture.environment)
                .env_remove("RUFLO_AGNTCY_SLIM_ENDPOINT")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(case.exit),
                "{binary} {:?}",
                case.argv
            );
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                case.stdout,
                "{binary} {:?} stdout",
                case.argv
            );
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                case.stderr,
                "{binary} {:?} stderr",
                case.argv
            );
            assert_eq!(
                std::fs::read_dir(project.path()).unwrap().count(),
                0,
                "transport wrote project files"
            );
        }
    }
}

fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|name| name == binary) {
        let status = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(status.success());
        built.push(binary.to_string());
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/debug")
        .join(binary)
}
