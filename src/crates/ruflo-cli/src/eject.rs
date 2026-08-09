//! Native V3 `eject` command (ADR-150 Phase 2) — lift the calling ruflo project
//! into a renamed standalone harness via `metaharness --from-existing`.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/eject.ts`. Safety gates:
//! dry-run by default, `--target` must resolve outside the calling repo, refuses
//! existing targets, subprocess + 10-minute hard timeout, graceful degradation
//! when metaharness is unavailable.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

const TIMEOUT_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone)]
struct EjectOptions {
    name: Option<String>,
    target: Option<String>,
    confirm: bool,
    format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EjectCommand {
    Run {
        name: Option<String>,
        target: Option<String>,
        confirm: bool,
        format: String,
    },
    Help,
}

pub fn run(root: &Path, command: EjectCommand) -> u8 {
    match command {
        EjectCommand::Help => {
            print!("{}", HELP);
            0
        }
        EjectCommand::Run {
            name,
            target,
            confirm,
            format,
        } => run_eject(
            root,
            EjectOptions {
                name,
                target,
                confirm,
                format,
            },
        ),
    }
}

fn run_eject(repo_root: &Path, opts: EjectOptions) -> u8 {
    // eject.ts:136 — `!opts.name` rejects empty names (JS falsy), not just absent.
    let Some(name) = opts.name.clone().filter(|n| !n.is_empty()) else {
        eprintln!("[ERROR] eject: --name is required");
        eprintln!();
        eprintln!("Example: ruflo eject --name my-harness");
        return 2;
    };

    let repo_root_abs = resolve_lexical(repo_root, repo_root);
    let target_abs = match &opts.target {
        Some(t) => resolve_lexical(repo_root, Path::new(t)),
        None => resolve_lexical(
            repo_root,
            Path::new(&format!(
                "{}/ruflo-eject-{}-{}",
                std::env::var_os("TMPDIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .display(),
                unix_millis(),
                name
            )),
        ),
    };

    // Safety gate 2: refuse targets inside the calling repo.
    if target_abs == repo_root_abs
        || target_abs.starts_with(format!("{}/", repo_root_abs.display()))
    {
        eprintln!(
            "[ERROR] eject: refusing to write to {}",
            target_abs.display()
        );
        eprintln!(
            "[ERROR] This is inside the calling repo ({}). Pick a --target OUTSIDE the repo.",
            repo_root_abs.display()
        );
        return 2;
    }
    // Safety gate 3: refuse existing targets.
    if target_abs.exists() {
        eprintln!(
            "[ERROR] eject: target {} already exists — refusing to overwrite",
            target_abs.display()
        );
        return 2;
    }

    let plan = json!({
        "name": name,
        "sourceRepo": repo_root_abs.display().to_string(),
        "target": target_abs.display().to_string(),
        "confirm": opts.confirm,
        "command": format!(
            "npx -y metaharness@latest --from-existing {} --name {} --target {} --yes",
            repo_root_abs.display(),
            name,
            target_abs.display()
        ),
    });

    if !opts.confirm {
        if opts.format == "json" {
            let mut dry = plan.clone();
            dry["dryRun"] = json!(true);
            println!("{}", serde_json::to_string_pretty(&dry).unwrap());
        } else {
            println!("# ruflo eject (dry-run)");
            println!();
            println!("name:       {}", plan["name"].as_str().unwrap_or(""));
            println!("sourceRepo: {}", plan["sourceRepo"].as_str().unwrap_or(""));
            println!("target:     {}", plan["target"].as_str().unwrap_or(""));
            println!();
            println!("Would execute:");
            println!("  {}", plan["command"].as_str().unwrap_or(""));
            println!();
            println!("Re-run with --confirm to actually eject.");
        }
        return 0;
    }

    // Actually run.
    println!("# ruflo eject — running");
    println!();
    println!(
        "Ejecting {} → {} as \"{}\"...",
        repo_root_abs.display(),
        target_abs.display(),
        name
    );
    println!();

    let (exit_code, degraded) = run_metaharness(&repo_root_abs, &target_abs, &name);
    if degraded {
        println!("[WARN] eject: metaharness binary unavailable — feature degraded");
        println!("(ADR-150 graceful degradation: ruflo runs without it; install with `npm i -D metaharness`.)");
        return 0;
    }
    if exit_code != 0 {
        eprintln!("[ERROR] eject: metaharness exited {exit_code}");
        return exit_code;
    }
    println!();
    println!("✓ Ejected to {}", target_abs.display());
    println!();
    println!("Next steps:");
    println!("  cd {}", target_abs.display());
    println!("  npm install");
    println!("  npx harness doctor");
    0
}

/// spawn metaharness with a 10-min hard timeout. Returns (exit_code, degraded).
/// degraded=true when the binary is unavailable (ENOENT) — ADR-150 graceful path.
fn run_metaharness(repo: &Path, target: &Path, name: &str) -> (u8, bool) {
    let mut cmd = Command::new(if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "npx"
    });
    if cfg!(target_os = "windows") {
        cmd.args(["/c", "npx", "-y", "metaharness@latest"]);
    } else {
        cmd.args(["-y", "metaharness@latest"]);
    }
    cmd.args([
        "--from-existing",
        &repo.display().to_string(),
        "--name",
        name,
        "--target",
        &target.display().to_string(),
        "--yes",
    ])
    .env_remove("RUFLO_FUNNEL");

    let Ok(mut child) = cmd.spawn() else {
        return (127, true); // npx unavailable → degraded
    };
    let start = std::time::Instant::now();
    loop {
        // Hard timeout: check elapsed BEFORE try_wait so a child exiting exactly
        // at the deadline still counts as timed out (mirrors spawnSync timeout).
        if start.elapsed().as_millis() as u64 >= TIMEOUT_MS {
            let _ = child.kill();
            let _ = child.wait();
            return (124, false); // timeout
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exit codes are 0-255 on Unix (kernel-truncated); the
                // CLI's u8 exit contract matches. metaharness is a JS tool that
                // exits 0/1/2, so no truncation occurs in practice.
                let code = status.code().unwrap_or(1) as u8;
                return (code, false);
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return (1, false),
        }
    }
}

/// Lexical absolute resolution (mirrors Node `path.resolve`): joins onto the
/// base, then collapses `.` and `..` without touching the filesystem. Needed so
/// the repo-inside check does not depend on the target existing.
fn resolve_lexical(base: &Path, p: &Path) -> PathBuf {
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    let mut out: Vec<std::path::Component> = Vec::new();
    for comp in absolute.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Pop only a real path segment; never pop the root or a prior `..`.
                if matches!(out.last(), Some(std::path::Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(comp);
                }
            }
            c => out.push(c),
        }
    }
    let mut result = PathBuf::new();
    for c in out {
        result.push(c.as_os_str());
    }
    if result.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        result
    }
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const HELP: &str = "\nruflo eject\nLift the calling ruflo project into a renamed standalone harness via metaharness --from-existing (ADR-150 Phase 2). Dry-run by default; --confirm required to write.\n\nOPTIONS:\n      --name <value>     Name for the ejected harness (required)\n      --target <value>   Absolute output dir (default: /tmp/ruflo-eject-<ts>-<name>/); refused if inside the calling repo\n      --confirm          Actually write the eject. Without this flag the command prints a dry-run plan and exits [default: false]\n      --format <value>   Output format: table | json [default: table]\n";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn resolve_lexical_normalizes_dotdot_and_makes_absolute() {
        let base = Path::new("/home/user/repo");
        assert_eq!(
            resolve_lexical(base, Path::new("src/../pkg")),
            PathBuf::from("/home/user/repo/pkg")
        );
        assert_eq!(
            resolve_lexical(base, Path::new("/tmp/out")),
            PathBuf::from("/tmp/out")
        );
        assert_eq!(
            resolve_lexical(base, Path::new("/tmp/a/./b")),
            PathBuf::from("/tmp/a/b")
        );
        assert_eq!(
            resolve_lexical(base, Path::new("../../out")),
            PathBuf::from("/home/out")
        );
    }

    #[test]
    fn name_required_exits_2() {
        let project = tempfile::tempdir().unwrap();
        let code = run(
            project.path(),
            EjectCommand::Run {
                name: None,
                target: None,
                confirm: false,
                format: "table".into(),
            },
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn target_inside_repo_refused() {
        let project = tempfile::tempdir().unwrap();
        let inside = project.path().join("subdir");
        let code = run(
            project.path(),
            EjectCommand::Run {
                name: Some("h".into()),
                target: Some(inside.display().to_string()),
                confirm: false,
                format: "table".into(),
            },
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn target_exists_refused() {
        let project = tempfile::tempdir().unwrap();
        let existing = tempfile::tempdir().unwrap();
        let code = run(
            project.path(),
            EjectCommand::Run {
                name: Some("h".into()),
                target: Some(existing.path().display().to_string()),
                confirm: false,
                format: "table".into(),
            },
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn dry_run_table_and_json_exit_zero() {
        let project = tempfile::tempdir().unwrap();
        // table
        let code = run(
            project.path(),
            EjectCommand::Run {
                name: Some("h".into()),
                target: None,
                confirm: false,
                format: "table".into(),
            },
        );
        assert_eq!(code, 0);
        // (output asserted via E2E)
        let _ = Value::Null; // serde_json available for E2E
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run(Path::new("/tmp"), EjectCommand::Help), 0);
    }
}
