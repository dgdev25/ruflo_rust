use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::fixture_schema::{CliFixture, Fixture, JsonRpcFixture};

#[allow(dead_code)]
pub fn assert_cli_fixture(binary: &str, fixture_path: &str) {
    let fixture = CliFixture::load(fixture_path).unwrap_or_else(|error| panic!("{error}"));
    let executable = executable_path(binary);

    let mut command = Command::new(&executable);
    command.args(fixture.argv.iter().map(OsString::from));
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn `{binary}` for fixture replay: {error}"));

    if let Some(stdin) = fixture.stdin.as_deref() {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }

    let output = child.wait_with_output().unwrap();
    let exit = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(
        exit, fixture.exit,
        "exit code mismatch for fixture `{fixture_path}`"
    );
    assert_eq!(
        stdout, fixture.stdout,
        "stdout mismatch for fixture `{fixture_path}`"
    );
    assert_eq!(
        stderr, fixture.stderr,
        "stderr mismatch for fixture `{fixture_path}`"
    );
}

pub fn assert_json_rpc_fixture(request: &Value, response: &Value, fixture_path: &str) {
    let fixture = JsonRpcFixture::load(fixture_path).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        request, &fixture.request,
        "request mismatch for `{fixture_path}`"
    );
    assert_eq!(
        response, &fixture.response,
        "response mismatch for `{fixture_path}`"
    );
}

#[allow(dead_code)]
fn cargo_bin_var(binary: &str) -> String {
    format!("CARGO_BIN_EXE_{}", binary.replace('-', "_"))
}

fn executable_path(binary: &str) -> PathBuf {
    if let Some(executable) = std::env::var_os(cargo_bin_var(binary)) {
        return executable.into();
    }

    build_workspace_binary(binary);
    target_debug_dir().join(format!("{binary}{}", std::env::consts::EXE_SUFFIX))
}

fn build_workspace_binary(binary: &str) {
    static BUILT_BINARIES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT_BINARIES
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if built.iter().any(|name| name == binary) {
        return;
    }

    let status = Command::new(env!("CARGO"))
        .current_dir(repo_root())
        .args([
            "build",
            "--quiet",
            "--package",
            binary_package(binary),
            "--bin",
            binary,
        ])
        .status()
        .unwrap_or_else(|error| panic!("failed to build `{binary}` for fixture replay: {error}"));

    assert!(
        status.success(),
        "failed to build `{binary}` for fixture replay"
    );
    built.push(binary.to_string());
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn binary_package(binary: &str) -> &'static str {
    match binary {
        "ruflo" => "ruflo",
        "claude-flow" => "claude-flow",
        "claude-flow-codex" => "claude-flow-codex",
        _ => panic!("no workspace package mapping registered for `{binary}`"),
    }
}

fn target_debug_dir() -> PathBuf {
    repo_root().join("target").join("debug")
}

#[test]
fn cli_fixture_requires_exit_stdout_and_stderr() {
    let parsed = Fixture::parse(
        r#"{
          "argv":["--version"],
          "exit":0,
          "stdout":"ruflo vX\n",
          "stderr":"",
          "environment":{},
          "platform":{"family":"portable"},
          "provenance":{"kind":"source-oracle","source":"managed:ruflo-cli","source_command":["--version"]},
          "recording":{"recorded_at":"2026-08-07T00:00:00Z","recorded_by":"test","harness":"differential-cli"}
        }"#,
    )
    .unwrap();
    assert_eq!(parsed.exit, 0);
}

#[test]
fn assert_json_rpc_fixture_replays_tools_list_contract() {
    let fixture = JsonRpcFixture::load("tests/fixtures/mcp/tools-list.json").unwrap();
    assert_json_rpc_fixture(
        &fixture.request,
        &fixture.response,
        "tests/fixtures/mcp/tools-list.json",
    );
}

#[test]
fn both_binaries_match_version_fixture() {
    assert_cli_fixture("ruflo", "tests/fixtures/cli/version.json");
    assert_cli_fixture("claude-flow", "tests/fixtures/cli/version.json");
}

#[test]
fn ruflo_matches_quiet_help_fixture() {
    assert_cli_fixture("ruflo", "tests/fixtures/cli/help.json");
}

#[test]
fn codex_facade_replays_safe_oracle_workflows() {
    for fixture in [
        "tests/fixtures/codex/version.json",
        "tests/fixtures/codex/dual-templates.json",
        "tests/fixtures/codex/dual-run-empty.json",
        "tests/fixtures/codex/dual-run-help.json",
    ] {
        assert_cli_fixture("claude-flow-codex", fixture);
    }
}

#[test]
fn both_binaries_replay_tools_list_fixture_over_stdio() {
    let fixture = JsonRpcFixture::load("tests/fixtures/mcp/tools-list.json").unwrap();

    for binary in ["ruflo", "claude-flow"] {
        let executable = executable_path(binary);
        let mut command = Command::new(executable);
        command.args(["mcp", "start"]);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("failed to spawn `{binary} mcp start`: {error}"));
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(format!("{}\n", fixture.request).as_bytes())
            .unwrap();

        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "unexpected exit code for `{binary} mcp start`"
        );

        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).is_ok()));
        assert_eq!(
            serde_json::from_str::<Value>(stdout.trim_end()).unwrap(),
            fixture.response,
            "unexpected tools/list response for `{binary} mcp start`"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "",
            "unexpected stderr for `{binary} mcp start`"
        );
    }
}
