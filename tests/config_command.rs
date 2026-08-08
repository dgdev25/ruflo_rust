use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct SequenceFixture {
    environment: BTreeMap<String, String>,
    cases: Vec<Case>,
    final_files: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct Case {
    argv: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
}

#[test]
fn both_native_aliases_replay_the_v3_config_sequence_and_filesystem_effects() {
    let fixture: SequenceFixture = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/cli/config/v3.json").unwrap(),
    )
    .unwrap();

    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let project_path = project.path().display().to_string();
        for case in &fixture.cases {
            let output = Command::new(executable(binary))
                .current_dir(project.path())
                .args(&case.argv)
                .envs(&fixture.environment)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(case.exit),
                "{binary} {:?}",
                case.argv
            );
            assert_eq!(
                normalize(&output.stdout, &project_path),
                case.stdout,
                "{binary} {:?} stdout",
                case.argv
            );
            assert_eq!(
                normalize(&output.stderr, &project_path),
                case.stderr,
                "{binary} {:?} stderr",
                case.argv
            );
        }

        for (relative, expected) in &fixture.final_files {
            let actual: Value = serde_json::from_str(
                &std::fs::read_to_string(project.path().join(relative)).unwrap(),
            )
            .unwrap();
            assert_eq!(
                &actual, expected,
                "{binary} filesystem effect for {relative}"
            );
        }
    }
}

fn normalize(bytes: &[u8], project: &str) -> String {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace(project, "<PROJECT>")
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
