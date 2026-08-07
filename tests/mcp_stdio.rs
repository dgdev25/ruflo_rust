use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

#[test]
fn tools_list_matches_checked_in_fixture() {
    let fixture = load_json("tests/fixtures/mcp/tools-list.json");
    let output = run_stdio("ruflo", &[fixture["request"].to_string().as_str()], &[]);

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);

    let frames = stdout_frames(&output.stdout);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0], fixture["response"]);
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn tools_call_dispatches_from_the_same_registry() {
    let output = run_stdio(
        "ruflo",
        &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"auth"}}}"#,
        ],
        &[],
    );

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);

    let frame = single_frame(&output.stdout);
    assert_eq!(frame["result"]["structuredContent"]["query"], "auth");
    assert_eq!(frame["result"]["structuredContent"]["matches"], json!([]));
    assert_eq!(
        frame["result"]["content"][0]["text"],
        "no stored matches for `auth`"
    );
}

#[test]
fn tools_call_matches_checked_in_fixture() {
    let fixture = load_json("tests/fixtures/mcp/memory-search-call.json");
    let project = tempfile::TempDir::new().unwrap();
    let output = run_stdio_in(
        "ruflo",
        &[fixture["request"].to_string().as_str()],
        &[],
        project.path(),
    );

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);
    assert_eq!(single_frame(&output.stdout), fixture["response"]);
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn memory_round_trip_matches_checked_in_fixture_and_creates_sqlite_store() {
    let fixture = load_json("tests/fixtures/mcp/memory-round-trip.json");
    let project = tempfile::TempDir::new().unwrap();
    let requests = fixture["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["request"].to_string())
        .collect::<Vec<_>>();
    let request_refs = requests.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_stdio_in("ruflo", &request_refs, &[], project.path());

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);
    let actual = stdout_frames(&output.stdout);
    let expected = fixture["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["response"].clone())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert!(project.path().join(".swarm").join("memory.db").is_file());
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn memory_retrieve_survives_a_new_mcp_process() {
    let fixture = load_json("tests/fixtures/mcp/memory-round-trip.json");
    let frames = fixture["frames"].as_array().unwrap();
    let project = tempfile::TempDir::new().unwrap();

    let store = run_stdio_in(
        "ruflo",
        &[frames[0]["request"].to_string().as_str()],
        &[],
        project.path(),
    );
    assert!(store.status.success());

    let retrieve = run_stdio_in(
        "ruflo",
        &[frames[1]["request"].to_string().as_str()],
        &[],
        project.path(),
    );
    assert!(retrieve.status.success());
    assert_eq!(single_frame(&retrieve.stdout), frames[1]["response"]);
}

#[test]
fn denied_tools_match_checked_in_fixture() {
    let fixture = load_json("tests/fixtures/mcp/memory-search-denied.json");
    let output = run_stdio(
        "ruflo",
        &[fixture["request"].to_string().as_str()],
        &[("RUFLO_MCP_DENY", "memory_search")],
    );

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);
    assert_eq!(single_frame(&output.stdout), fixture["response"]);
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn denied_tool_is_hidden_from_discovery_and_invocation() {
    let output = run_stdio(
        "ruflo",
        &[
            r#"{"jsonrpc":"2.0","id":"list","method":"tools/list","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":"call","method":"tools/call","params":{"name":"memory_search","arguments":{"query":"auth"}}}"#,
        ],
        &[("RUFLO_MCP_DENY", "memory_search")],
    );

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);

    let frames = stdout_frames(&output.stdout);
    let tools = frames[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        tools,
        vec!["agent_spawn", "memory_store", "memory_retrieve"]
    );
    assert_eq!(frames[1]["error"]["code"], -32001);
    assert_eq!(
        frames[1]["error"]["data"]["details"]["capability"],
        "memory.search"
    );
    assert!(frames[1]["error"]["data"]["correlationId"]
        .as_str()
        .unwrap()
        .starts_with("corr-"));
}

#[test]
fn stdout_contains_only_newline_delimited_jsonrpc_frames() {
    let output = run_stdio(
        "ruflo",
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"agent_spawn","arguments":{"role":"coder"}}}"#,
        ],
        &[],
    );

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn parse_and_dispatch_errors_map_to_stable_error_objects() {
    let output = run_stdio(
        "ruflo",
        &[
            "not json",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"missing_tool","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":7}}}"#,
        ],
        &[],
    );

    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);

    let frames = stdout_frames(&output.stdout);
    assert_eq!(frames[0]["error"]["code"], -32700);
    assert_eq!(frames[1]["error"]["code"], -32602);
    assert_eq!(
        frames[1]["error"]["data"]["details"]["code"],
        "tool.not_found"
    );
    assert_eq!(frames[2]["error"]["code"], -32602);
    assert_eq!(
        frames[2]["error"]["data"]["details"]["code"],
        "tool.invalid_arguments"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("mcp parse error:"));
}

fn load_json(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo_root().join(path)).unwrap()).unwrap()
}

fn single_frame(stdout: &[u8]) -> Value {
    let mut frames = stdout_frames(stdout);
    assert_eq!(frames.len(), 1);
    frames.remove(0)
}

fn stdout_frames(stdout: &[u8]) -> Vec<Value> {
    let text = String::from_utf8(stdout.to_vec()).unwrap();
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

fn assert_stdout_is_jsonrpc_only(stdout: &[u8]) {
    let text = String::from_utf8(stdout.to_vec()).unwrap();
    assert!(text.ends_with('\n'));
    for line in text.lines() {
        let parsed = serde_json::from_str::<Value>(line).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
    }
}

fn run_stdio(binary: &str, input_lines: &[&str], env: &[(&str, &str)]) -> Output {
    run_stdio_in(binary, input_lines, env, repo_root())
}

fn run_stdio_in(
    binary: &str,
    input_lines: &[&str],
    env: &[(&str, &str)],
    workdir: &Path,
) -> Output {
    let executable = executable_path(binary);
    let mut command = Command::new(executable);
    command.arg("mcp").arg("start");
    command.current_dir(workdir);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.envs(
        env.iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );

    let mut child = command.spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for line in input_lines {
            stdin.write_all(line.as_bytes()).unwrap();
            stdin.write_all(b"\n").unwrap();
        }
    }

    child.wait_with_output().unwrap()
}

fn executable_path(binary: &str) -> PathBuf {
    if let Some(executable) = std::env::var_os(cargo_bin_var(binary)) {
        return executable.into();
    }

    build_workspace_binary(binary);
    repo_root()
        .join("target")
        .join("debug")
        .join(format!("{binary}{}", std::env::consts::EXE_SUFFIX))
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
        .unwrap();

    assert!(status.success());
    built.push(binary.to_string());
}

fn cargo_bin_var(binary: &str) -> String {
    format!("CARGO_BIN_EXE_{}", binary.replace('-', "_"))
}

fn binary_package(binary: &str) -> &'static str {
    match binary {
        "ruflo" => "ruflo",
        "claude-flow" => "claude-flow",
        _ => panic!("no package mapping for `{binary}`"),
    }
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}
