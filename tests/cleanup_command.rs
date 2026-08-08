use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    environment: std::collections::BTreeMap<String, String>,
    cases: Vec<FixtureCase>,
}
#[derive(Deserialize)]
struct FixtureCase {
    argv: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
}

#[test]
fn both_aliases_replay_source_cleanup_empty_help_and_unknown_flag_fixtures() {
    let fixture: Fixture =
        serde_json::from_str(&fs::read_to_string("tests/fixtures/cli/cleanup/v3.json").unwrap())
            .unwrap();
    for binary in ["ruflo", "claude-flow"] {
        for case in &fixture.cases {
            let project = tempfile::tempdir().unwrap();
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
        }
    }
}

#[test]
fn cleanup_is_dry_run_by_default_and_force_is_project_scoped_for_both_aliases() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        seed_all_candidates(project.path());
        let before = snapshot(project.path());

        let dry = run(binary, project.path(), &["cleanup"]);
        assert_eq!(dry.status.code(), Some(0));
        let stdout = String::from_utf8(dry.stdout).unwrap();
        assert_eq!(stdout, populated_dry_run_output());
        assert!(dry.stderr.is_empty());
        assert_eq!(
            snapshot(project.path()),
            before,
            "dry run changed {binary} tree"
        );

        let forced = run(binary, project.path(), &["cleanup", "--force", "--dry-run"]);
        assert_eq!(forced.status.code(), Some(0));
        assert_eq!(
            String::from_utf8(forced.stdout).unwrap(),
            populated_force_output()
        );
        assert!(forced.stderr.is_empty());
        for removed in [
            ".claude/helpers",
            ".claude-flow",
            "data",
            ".swarm",
            ".hive-mind",
            "coordination",
            "memory",
            "claude-flow.config.json",
        ] {
            assert!(
                !project.path().join(removed).exists(),
                "{binary} retained {removed}"
            );
        }
        assert!(project.path().join(".claude/agents/user.md").is_file());
        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.path().join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["theme"], "dark");
        assert!(settings.get("hooks").is_none());
        assert!(settings.get("claudeFlow").is_none());
    }
}

fn populated_dry_run_output() -> String {
    "\nClaude Flow Cleanup (dry run)\n\nArtifacts found:\n\n  [would remove] dir   .claude/helpers  (1 B) - Ruflo hook scripts\n  [would remove] file  .claude/settings.json  (0 B) - Remove ruflo hooks/claudeFlow blocks (preserves rest)\n  [would remove] dir   .claude-flow  (1 B) - Capabilities and configuration\n  [would remove] dir   data  (1 B) - Memory databases\n  [would remove] dir   .swarm  (1 B) - Swarm state\n  [would remove] dir   .hive-mind  (1 B) - Consensus state\n  [would remove] dir   coordination  (1 B) - Coordination data\n  [would remove] dir   memory  (1 B) - Memory storage\n  [would remove] file  claude-flow.config.json  (3 B) - Claude Flow configuration\n\nSummary:\n  Found 9 artifact(s) totaling 10 B\n\n  This was a dry run. Use --force to actually remove artifacts.\n\n".into()
}

fn populated_force_output() -> String {
    populated_dry_run_output()
        .replace("Claude Flow Cleanup (dry run)", "Claude Flow Cleanup")
        .replace("[would remove]", "[removed]")
        .replace(
            "  Found 9 artifact(s) totaling 10 B\n\n  This was a dry run. Use --force to actually remove artifacts.",
            "  Removed 9 artifact(s) totaling 10 B",
        )
}

#[test]
fn cleanup_keep_config_and_alias_preserve_configuration_bytes() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        seed_all_candidates(project.path());
        let settings = fs::read(project.path().join(".claude/settings.json")).unwrap();
        let config = fs::read(project.path().join("claude-flow.config.json")).unwrap();
        let output = run(binary, project.path(), &["clean", "-f", "-k"]);
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("[skip] file  .claude/settings.json"));
        assert!(stdout.contains("Preserved 2 item(s) (--keep-config)"));
        assert_eq!(
            fs::read(project.path().join(".claude/settings.json")).unwrap(),
            settings
        );
        assert_eq!(
            fs::read(project.path().join("claude-flow.config.json")).unwrap(),
            config
        );
        assert!(project.path().join(".claude/agents/user.md").is_file());
    }
}

fn seed_all_candidates(root: &Path) {
    for directory in [
        ".claude/helpers",
        ".claude/agents",
        ".claude-flow",
        "data",
        ".swarm",
        ".hive-mind",
        "coordination",
        "memory",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join(".claude/helpers/hook"), "x").unwrap();
    fs::write(root.join(".claude/agents/user.md"), "keep").unwrap();
    fs::write(root.join(".claude/settings.json"), "{\n  \"hooks\": {\"ruflo\": true},\n  \"claudeFlow\": {\"enabled\": true},\n  \"theme\": \"dark\"\n}\n").unwrap();
    for path in [
        ".claude-flow/state",
        "data/state",
        ".swarm/state",
        ".hive-mind/state",
        "coordination/state",
        "memory/state",
    ] {
        fs::write(root.join(path), "x").unwrap();
    }
    fs::write(root.join("claude-flow.config.json"), "{}\n").unwrap();
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries = fs::read_dir(current)
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files
}

fn run(binary: &str, project: &Path, args: &[&str]) -> std::process::Output {
    Command::new(executable(binary))
        .current_dir(project)
        .args(args)
        .env("RUFLO_DAEMON_AUTOSTART", "0")
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
        assert!(status.success());
        built.push(binary.to_string());
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/debug")
        .join(binary)
}
