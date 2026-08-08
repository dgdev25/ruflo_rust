//! End-to-end `completions` command tests through both native binaries.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/completions.ts`. Asserts the
//! overview action, the four shell scripts (bash/zsh/fish/powershell), the `pwsh`
//! alias, and that both binaries emit identical output.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

const TOP_LEVEL: &[&str] = &[
    "swarm",
    "agent",
    "task",
    "session",
    "config",
    "memory",
    "workflow",
    "hive-mind",
    "hooks",
    "daemon",
    "neural",
    "security",
    "performance",
    "providers",
    "plugins",
    "deployment",
    "claims",
    "embeddings",
    "doctor",
    "completions",
    "help",
    "version",
];

#[test]
fn overview_action_when_no_shell_given() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["completions"]);
    let b = run("claude-flow", project.path(), &["completions"]);
    assert_success(&a);
    assert_success(&b);
    assert_eq!(stdout(&a), stdout(&b));
    assert!(stdout(&a).contains("Shell Completions"));
    assert!(stdout(&a).contains("powershell"));
}

#[test]
fn bash_script_matches_ts_structure_across_both_binaries() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["completions", "bash"]);
    let b = run("claude-flow", project.path(), &["completions", "bash"]);
    assert_success(&a);
    assert_eq!(
        stdout(&a),
        stdout(&b),
        "both binaries must emit identical bash script"
    );
    let out = stdout(&a);
    assert!(out.contains("# claude-flow bash completion"));
    assert!(out.contains("_claude_flow_completions()"));
    assert!(out.contains("complete -F _claude_flow_completions claude-flow"));
    assert!(out.contains("npx\\ @claude-flow/cli@v3alpha"));
    for cmd in TOP_LEVEL {
        assert!(
            out.contains(cmd),
            "bash script missing top-level command {cmd}"
        );
    }
    // subcommand groups
    assert!(out.contains("init status scale destroy monitor optimize")); // swarm
    assert!(out.contains("list check grant revoke roles policies")); // claims
}

#[test]
fn zsh_script_matches_ts_structure_across_both_binaries() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["completions", "zsh"]);
    let b = run("claude-flow", project.path(), &["completions", "zsh"]);
    assert_success(&a);
    assert_eq!(stdout(&a), stdout(&b));
    let out = stdout(&a);
    assert!(out.contains("#compdef claude-flow"));
    assert!(out.contains("_claude_flow()"));
    assert!(out.contains("_describe -t commands 'claude-flow commands' commands"));
    assert!(out.contains("'memory:Memory operations with AgentDB'"));
    assert!(out.contains("hive-mind|hive)"));
}

#[test]
fn fish_script_matches_ts_structure_across_both_binaries() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["completions", "fish"]);
    let b = run("claude-flow", project.path(), &["completions", "fish"]);
    assert_success(&a);
    assert_eq!(stdout(&a), stdout(&b));
    let out = stdout(&a);
    assert!(out.contains("# claude-flow fish completion"));
    assert!(out.contains("complete -c claude-flow -f"));
    assert!(out.contains("__fish_seen_subcommand_from hive-mind hive"));
    assert!(out.contains("__fish_seen_subcommand_from deployment deploy"));
    for cmd in TOP_LEVEL {
        assert!(out.contains(&format!("-a \"{cmd}\"")), "fish missing {cmd}");
    }
}

#[test]
fn powershell_script_and_pwsh_alias_match_ts_structure() {
    let project = tempfile::tempdir().unwrap();
    let ps = run("ruflo", project.path(), &["completions", "powershell"]);
    let pwsh = run("claude-flow", project.path(), &["completions", "pwsh"]);
    assert_success(&ps);
    assert_success(&pwsh);
    // pwsh alias must produce the identical powershell script.
    assert_eq!(stdout(&ps), stdout(&pwsh));
    let out = stdout(&ps);
    assert!(out.contains("# claude-flow PowerShell completion"));
    assert!(out.contains("$script:ClaudeFlowCommands"));
    assert!(out.contains("$script:SubCommands"));
    assert!(out.contains("Register-ArgumentCompleter -Native -CommandName claude-flow"));
    assert!(out.contains("'hive' = @('init'"));
}

#[test]
fn unsupported_shell_exits_nonzero() {
    let project = tempfile::tempdir().unwrap();
    let bad = run("ruflo", project.path(), &["completions", "tcsh"]);
    assert_ne!(bad.status.code(), Some(0));
    assert!(stderr(&bad).contains("unsupported shell"));
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

fn run(binary: &str, root: &Path, args: &[&str]) -> Output {
    Command::new(executable(binary))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
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
