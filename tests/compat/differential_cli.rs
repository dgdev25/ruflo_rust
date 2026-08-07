use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::fixture_schema::{CliFixture, Fixture, JsonRpcFixture};

#[allow(dead_code)]
pub fn assert_cli_fixture(binary: &str, fixture_path: &str) {
    let fixture = CliFixture::load(fixture_path).unwrap_or_else(|error| panic!("{error}"));
    let executable = std::env::var_os(cargo_bin_var(binary)).unwrap_or_else(|| {
        panic!(
            "binary `{binary}` is not built for this test target yet; run this helper after Task 4 adds the native CLI binaries"
        )
    });

    let mut command = Command::new(executable);
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
