use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};

use ruflo_runtime::{NewTask, Runtime, TaskState};
use ruflo_storage::PersistencePort;
use serde::Deserialize;
use serde_json::Value;
use tempfile::TempDir;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct HookFixture {
    platform: String,
    executable: String,
    args: Vec<String>,
    forbidden_fragments: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum TargetPlatform {
    Posix,
    Windows,
}

#[test]
fn generated_posix_hook_matches_checked_in_fixture() {
    assert_eq!(
        generate_hook(TargetPlatform::Posix),
        load_hook_fixture("tests/fixtures/consumers/platform-hooks/posix.json")
    );
}

#[test]
fn generated_windows_hook_matches_checked_in_fixture() {
    assert_eq!(
        generate_hook(TargetPlatform::Windows),
        load_hook_fixture("tests/fixtures/consumers/platform-hooks/windows.json")
    );
}

#[test]
fn generated_hooks_use_tokenized_native_arguments_not_shell_pipelines() {
    for target in [TargetPlatform::Posix, TargetPlatform::Windows] {
        let hook = generate_hook(target);
        let rendered = format!("{} {}", hook.executable, hook.args.join(" "));

        assert_eq!(hook.args, vec!["mcp", "start"]);
        assert!(rendered.contains("ruflo"));

        for fragment in &hook.forbidden_fragments {
            assert!(
                !rendered.contains(fragment),
                "rendered hook `{rendered}` should not contain `{fragment}`"
            );
        }
    }
}

#[test]
fn local_platform_smoke_covers_aliases_stdio_locks_and_cancellation() {
    let version_fixture = load_json("tests/fixtures/cli/version.json");
    let expected_version = version_fixture["stdout"].as_str().unwrap();

    for binary in ["ruflo", "claude-flow"] {
        let output = run_binary(binary, &["--version"]);
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected_version);
        assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
    }

    let output = run_stdio(
        "ruflo",
        &[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#],
    );
    assert!(output.status.success());
    assert_stdout_is_jsonrpc_only(&output.stdout);

    let frame = single_frame(&output.stdout);
    assert_eq!(frame["jsonrpc"], "2.0");
    assert!(frame["result"]["tools"].is_array());

    let project = TestProject::new();
    let port = PersistencePort::open(project.root(), project.database_path()).unwrap();
    let session = port.begin_migration().unwrap();
    assert!(project.lock_path().exists());
    drop(session);
    assert!(!project.lock_path().exists());

    let runtime = Runtime::ephemeral();
    let task = runtime
        .create_task(NewTask::named("platform-smoke-task"))
        .unwrap();
    let cancelled = runtime.cancel_task(task.id).unwrap();
    assert_eq!(cancelled.state, TaskState::Cancelled);
}

fn generate_hook(target: TargetPlatform) -> HookFixture {
    match target {
        TargetPlatform::Posix => HookFixture {
            platform: "posix".to_string(),
            executable: "ruflo".to_string(),
            args: vec!["mcp".to_string(), "start".to_string()],
            forbidden_fragments: vec![
                "|".to_string(),
                "/bin/bash".to_string(),
                "cmd.exe".to_string(),
            ],
        },
        TargetPlatform::Windows => HookFixture {
            platform: "windows".to_string(),
            executable: "ruflo.exe".to_string(),
            args: vec!["mcp".to_string(), "start".to_string()],
            forbidden_fragments: vec![
                "|".to_string(),
                "/bin/bash".to_string(),
                "cmd.exe /c".to_string(),
            ],
        },
    }
}

fn load_hook_fixture(path: &str) -> HookFixture {
    serde_json::from_str(&fs::read_to_string(repo_root().join(path)).unwrap()).unwrap()
}

fn load_json(path: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(repo_root().join(path)).unwrap()).unwrap()
}

fn single_frame(stdout: &[u8]) -> Value {
    let frames = stdout_frames(stdout);
    assert_eq!(frames.len(), 1);
    frames.into_iter().next().unwrap()
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

fn run_binary(binary: &str, args: &[&str]) -> Output {
    let executable = executable_path(binary);
    Command::new(executable).args(args).output().unwrap()
}

fn run_stdio(binary: &str, input_lines: &[&str]) -> Output {
    let executable = executable_path(binary);
    let mut command = Command::new(executable);
    command.arg("mcp").arg("start");
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

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
    if let Some(executable) = std::env::var_os(smoke_bin_var(binary)) {
        return executable.into();
    }

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

    let mut command = Command::new(env!("CARGO"));
    command.current_dir(repo_root()).args([
        "build",
        "--quiet",
        "--package",
        binary_package(binary),
        "--bin",
        binary,
    ]);
    // The distributed Windows CLI intentionally omits ONNX Runtime because
    // its bundled native library has an incompatible C runtime contract with
    // the MSVC dependency graph. The no-ONNX implementation exposes the same
    // safe API and fails closed for BGE-specific operations.
    if cfg!(target_os = "windows") {
        command.arg("--no-default-features");
    }
    let status = command.status().unwrap();

    assert!(status.success());
    built.push(binary.to_string());
}

fn cargo_bin_var(binary: &str) -> String {
    format!("CARGO_BIN_EXE_{}", binary.replace('-', "_"))
}

/// An explicit executable takes precedence in release CI so this smoke test
/// exercises the same feature set that will be archived. In particular, the
/// Windows release intentionally omits static ONNX Runtime and must never
/// trigger an incidental default-feature child build.
fn smoke_bin_var(binary: &str) -> String {
    format!(
        "RUFLO_SMOKE_BIN_{}",
        binary.replace('-', "_").to_uppercase()
    )
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

struct TestProject {
    root: TempDir,
    database_path: PathBuf,
}

impl TestProject {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let database_path = root.path().join("legacy-empty.db");
        fs::copy("tests/fixtures/persistence/legacy-empty.db", &database_path).unwrap();
        Self {
            root,
            database_path,
        }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn database_path(&self) -> &Path {
        &self.database_path
    }

    fn lock_path(&self) -> PathBuf {
        self.root.path().join(".ruflo-storage.lock")
    }
}
