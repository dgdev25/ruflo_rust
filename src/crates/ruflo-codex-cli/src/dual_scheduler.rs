//! Native, opt-in runner for the Codex portion of Ruflo's dual-mode CLI.
//!
//! The scheduler deliberately has no shell escape hatch: worker prompts are
//! passed as one positional argument to `codex exec`, and every writer gets a
//! dedicated Git worktree retained for explicit review/integration.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ruflo_storage::{MemoryStoreInput, SqliteMemoryStore};
use serde::Serialize;

const BANNER: &str = "═══════════════════════════════════════════════════════════════\n  DUAL-MODE COLLABORATIVE EXECUTION\n  Claude Code + Codex workers with shared memory\n═══════════════════════════════════════════════════════════════\n\n";
const DEFAULT_MAX_CONCURRENT: usize = 4;
const DEFAULT_MAX_WRITERS: usize = 2;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DEFAULT_MAX_OUTPUT_BYTES: usize = 1_048_576;

type ChildOutput = (Option<i32>, Vec<u8>, Vec<u8>);

#[derive(Debug, Clone)]
struct AutomationConfig {
    enabled: bool,
    max_concurrent: usize,
    max_writers: usize,
    timeout: Duration,
    max_output_bytes: usize,
    worktree_isolation: bool,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            max_writers: DEFAULT_MAX_WRITERS,
            timeout: DEFAULT_TIMEOUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            worktree_isolation: true,
        }
    }
}

#[derive(Debug, Clone)]
struct Worker {
    id: String,
    role: String,
    prompt: String,
    read_only: bool,
}

#[derive(Debug, Serialize)]
struct WorktreeRecord {
    version: u8,
    run_id: String,
    repo_root: String,
    created_at: u128,
    assignments: Vec<WorktreeAssignment>,
}

#[derive(Debug, Clone, Serialize)]
struct WorktreeAssignment {
    agent_id: String,
    branch: String,
    path: String,
    read_only: bool,
}

pub(crate) fn has_worker_invocation(args: &[String]) -> bool {
    args.iter()
        .any(|argument| argument == "--worker" || argument == "-w")
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    print!("{BANNER}");

    let project_root = match env::current_dir().and_then(fs::canonicalize) {
        Ok(path) => path,
        Err(error) => return fail(format!("failed to resolve project root: {error}")),
    };
    let automation = match AutomationConfig::load(&project_root) {
        Ok(config) => config,
        Err(error) => return fail(error),
    };
    if !automation.enabled {
        return fail(
            "unattended swarm automation is disabled; set [swarm.automation] enabled = true".into(),
        );
    }

    let workers = match parse_workers(args) {
        Ok(workers) => workers,
        Err(error) => return fail(error),
    };
    if workers.iter().any(|worker| worker.role.trim().is_empty()) {
        return fail("worker role must not be empty".into());
    }
    if let Some(worker) = workers.iter().find(|worker| !is_safe_id(&worker.id)) {
        return fail(format!("invalid worker id `{}`", worker.id));
    }

    let requested_timeout = option_value(args, "--timeout")
        .map(|value| parse_positive(&value, "--timeout"))
        .transpose();
    let timeout = match requested_timeout {
        Ok(Some(value)) => Duration::from_millis(value).min(automation.timeout),
        Ok(None) => automation.timeout,
        Err(error) => return fail(error),
    };
    let requested_concurrency = option_value(args, "--max-concurrent")
        .map(|value| parse_positive(&value, "--max-concurrent"))
        .transpose();
    let max_concurrent = match requested_concurrency {
        Ok(Some(value)) => usize::try_from(value)
            .unwrap_or(automation.max_concurrent)
            .min(automation.max_concurrent),
        Ok(None) => automation.max_concurrent,
        Err(error) => return fail(error),
    };
    let namespace =
        option_value(args, "--namespace").unwrap_or_else(|| "collaboration".to_string());
    if namespace.trim().is_empty() {
        return fail("--namespace must not be empty".into());
    }

    let run_id = format!("dual-{}", unique_suffix());
    let assignments = if automation.worktree_isolation {
        match prepare_worktrees(&project_root, &run_id, &workers) {
            Ok(assignments) => assignments,
            Err(error) => return fail(error),
        }
    } else {
        workers
            .iter()
            .map(|worker| WorktreeAssignment {
                agent_id: worker.id.clone(),
                branch: String::new(),
                path: project_root.display().to_string(),
                read_only: worker.read_only,
            })
            .collect()
    };

    if let Err(error) = initialize_memory(&project_root, &namespace, &workers) {
        return fail(error);
    }

    println!("  Worktree run: {run_id} (retained for explicit integrate/cleanup)");
    println!();
    println!("Swarm Configuration:");
    println!("  Workers: {}", workers.len());
    println!("  Max Concurrent: {max_concurrent}");
    println!("  Timeout: {}ms", timeout.as_millis());
    println!("  Namespace: {namespace}");
    println!();
    println!("Worker Pipeline:");
    for worker in &workers {
        println!("  🟢 {}: {}", worker.id, worker.role);
    }
    println!();
    println!("Starting collaboration...");
    println!();

    // Worker specs are sequential by default. `--parallel-workers` is
    // conservatively partitioned: writers never exceed the configured writer
    // cap and every child retains its own worktree.
    let parallel = args.iter().any(|argument| argument == "--parallel-workers");
    let batch_size = if parallel {
        max_concurrent.min(automation.max_writers).max(1)
    } else {
        1
    };
    let mut failures = Vec::new();
    for batch in workers.chunks(batch_size) {
        let mut handles = Vec::new();
        for worker in batch {
            let assignment = assignments
                .iter()
                .find(|assignment| assignment.agent_id == worker.id)
                .expect("each worker has an assignment")
                .path
                .clone();
            let worker = worker.clone();
            let namespace = namespace.clone();
            handles.push(thread::spawn(move || {
                execute_worker(
                    &worker,
                    Path::new(&assignment),
                    &namespace,
                    timeout,
                    automation.max_output_bytes,
                )
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(Ok(worker)) => println!("✓ Worker {} completed", worker.id),
                Ok(Err(error)) => {
                    eprintln!("✗ Worker failed: {error}");
                    failures.push(error);
                }
                Err(_) => failures.push("worker scheduler thread panicked".into()),
            }
        }
        if !failures.is_empty() {
            break;
        }
    }

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("  COLLABORATION COMPLETE");
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    if failures.is_empty() {
        println!("Results:");
        println!("  Status: SUCCESS");
        ExitCode::SUCCESS
    } else {
        eprintln!("error: collaboration failed: {}", failures.join("; "));
        ExitCode::from(1)
    }
}

impl AutomationConfig {
    fn load(project_root: &Path) -> Result<Self, String> {
        let file = [
            project_root.join(".agents/config.toml"),
            project_root.join(".codex/config.toml"),
        ]
        .into_iter()
        .find(|path| path.is_file());
        let Some(file) = file else {
            return Ok(Self::default());
        };
        let content = fs::read_to_string(&file)
            .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
        let mut config = Self::default();
        let mut section = "";
        for raw in content.lines() {
            let line = raw.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
                continue;
            }
            if section != "swarm.automation" {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("invalid automation config line `{line}`"));
            };
            let value = value.trim();
            match key.trim() {
                "enabled" => config.enabled = parse_bool(value, "enabled")?,
                "worktree_isolation" => {
                    config.worktree_isolation = parse_bool(value, "worktree_isolation")?
                }
                "max_concurrent" => config.max_concurrent = parse_usize(value, "max_concurrent")?,
                "max_writers" => config.max_writers = parse_usize(value, "max_writers")?,
                "agent_timeout_seconds" => {
                    config.timeout =
                        Duration::from_secs(parse_positive(value, "agent_timeout_seconds")?)
                }
                "max_output_bytes" => {
                    config.max_output_bytes = parse_usize(value, "max_output_bytes")?
                }
                _ => {}
            }
        }
        Ok(config)
    }
}

fn parse_workers(args: &[String]) -> Result<Vec<Worker>, String> {
    let mut workers = Vec::new();
    let mut used_ids = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] != "--worker" && args[index] != "-w" {
            index += 1;
            continue;
        }
        let spec = args
            .get(index + 1)
            .ok_or_else(|| "--worker requires a value".to_string())?;
        let mut parts = spec.splitn(3, ':');
        let platform = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
        let role = parts.next().unwrap_or_default().trim();
        let prompt = parts.next().unwrap_or_default().trim();
        if platform != "codex" {
            return Err(format!("native dual-run currently supports only `codex` workers; `{platform}` requires its own native scheduler"));
        }
        if role.is_empty() || prompt.is_empty() {
            return Err(format!("Invalid --worker spec \"{spec}\". Expected \"<platform>:<role>:<prompt>\" (platform = claude|codex)."));
        }
        let base = role
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .to_ascii_lowercase();
        let mut id = base.clone();
        let mut suffix = 2;
        while used_ids.contains(&id) {
            id = format!("{base}-{suffix}");
            suffix += 1;
        }
        used_ids.insert(id.clone());
        workers.push(Worker {
            id,
            role: role.to_string(),
            prompt: prompt.to_string(),
            read_only: is_read_only_role(role),
        });
        index += 2;
    }
    if workers.is_empty() {
        return Err("native dual-run requires at least one --worker".into());
    }
    Ok(workers)
}

fn initialize_memory(
    project_root: &Path,
    namespace: &str,
    workers: &[Worker],
) -> Result<(), String> {
    let store = SqliteMemoryStore::open(project_root, project_root.join(".swarm/memory.db"))
        .map_err(|error| format!("failed to initialize shared memory: {error}"))?;
    let task_context = format!(
        "Custom dual-mode swarm: {}",
        workers
            .iter()
            .map(|worker| format!("codex:{}", worker.role))
            .collect::<Vec<_>>()
            .join(" -> ")
    );
    store
        .store(&MemoryStoreInput {
            key: "task-context".into(),
            namespace: namespace.into(),
            content: task_context,
            memory_type: "semantic".into(),
            tags_json: None,
            provenance_type: "system_observation".into(),
            upsert: true,
        })
        .map_err(|error| format!("failed to store shared task context: {error}"))?;
    println!("✓ Shared memory initialized: {namespace}");
    Ok(())
}

fn prepare_worktrees(
    project_root: &Path,
    run_id: &str,
    workers: &[Worker],
) -> Result<Vec<WorktreeAssignment>, String> {
    let top = git(project_root, ["rev-parse", "--show-toplevel"])?;
    let repository = fs::canonicalize(top.trim())
        .map_err(|error| format!("failed to resolve git root: {error}"))?;
    if repository != project_root {
        return Err(format!(
            "project root must be the Git top-level: {}",
            repository.display()
        ));
    }
    if !git(project_root, ["status", "--porcelain"])?
        .trim()
        .is_empty()
    {
        return Err("refusing to prepare writing worktrees from a dirty repository".into());
    }
    let worktree_base = repository
        .parent()
        .unwrap_or(&repository)
        .join(".ruflo-worktrees")
        .join(repository.file_name().unwrap_or_default())
        .join(run_id);
    let registry_dir = repository.join(".claude-flow/swarm/worktrees");
    fs::create_dir_all(&worktree_base)
        .map_err(|error| format!("failed to create worktree directory: {error}"))?;
    fs::create_dir_all(&registry_dir)
        .map_err(|error| format!("failed to create worktree registry: {error}"))?;
    let mut assignments = Vec::new();
    for worker in workers {
        let path = worktree_base.join(&worker.id);
        let mut arguments = vec!["worktree".into(), "add".into()];
        let branch = if worker.read_only {
            arguments.push("--detach".into());
            String::new()
        } else {
            let branch = format!("ruflo/{run_id}/{}", worker.id);
            arguments.extend(["-b".into(), branch.clone()]);
            branch
        };
        arguments.push(path.display().to_string());
        arguments.push("HEAD".into());
        if let Err(error) = git_owned(project_root, arguments) {
            cleanup_partial_worktrees(project_root, &assignments);
            return Err(error);
        }
        assignments.push(WorktreeAssignment {
            agent_id: worker.id.clone(),
            branch,
            path: path.display().to_string(),
            read_only: worker.read_only,
        });
    }
    let record = WorktreeRecord {
        version: 1,
        run_id: run_id.into(),
        repo_root: repository.display().to_string(),
        created_at: unique_suffix(),
        assignments: assignments.clone(),
    };
    let record_path = registry_dir.join(format!("{run_id}.json"));
    let temporary = record_path.with_extension("json.tmp");
    fs::write(
        &temporary,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?
        ),
    )
    .map_err(|error| format!("failed to write worktree registry: {error}"))?;
    fs::rename(&temporary, &record_path)
        .map_err(|error| format!("failed to commit worktree registry: {error}"))?;
    Ok(assignments)
}

/// A failed prepare must not leave a half-created set of writer worktrees or
/// their private branches behind. Every target here was created by this run;
/// errors during cleanup deliberately preserve the original prepare failure.
fn cleanup_partial_worktrees(project_root: &Path, assignments: &[WorktreeAssignment]) {
    for assignment in assignments.iter().rev() {
        let _ = git_owned(
            project_root,
            vec!["worktree".into(), "remove".into(), assignment.path.clone()],
        );
        if !assignment.branch.is_empty() {
            let _ = git_owned(
                project_root,
                vec!["branch".into(), "-D".into(), assignment.branch.clone()],
            );
        }
    }
}

fn execute_worker(
    worker: &Worker,
    worktree: &Path,
    namespace: &str,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<Worker, String> {
    let prompt = collaborative_prompt(worker, worktree, namespace);
    let executable =
        env::var_os("RUFLO_CODEX_EXECUTABLE").unwrap_or_else(|| OsString::from("codex"));
    let mut command = Command::new(executable);
    command
        .args([
            "exec",
            "--sandbox",
            if worker.read_only {
                "read-only"
            } else {
                "workspace-write"
            },
            "--skip-git-repo-check",
            &prompt,
        ])
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, _) in env::vars() {
        if is_sensitive_environment(&name) {
            command.env_remove(name);
        }
    }
    let child = command
        .spawn()
        .map_err(|error| format!("worker {} failed to start Codex CLI: {error}", worker.id))?;
    let (status, stdout, stderr) = wait_for_output(child, timeout, max_output_bytes)?;
    if status != Some(0) {
        return Err(format!(
            "worker {} exited with code {}: {}",
            worker.id,
            status.map_or_else(|| "signal".into(), |code| code.to_string()),
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    let _ = stdout; // Receipt output remains bounded and is intentionally not mixed into CLI presentation.
    Ok(worker.clone())
}

fn wait_for_output(
    mut child: Child,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<ChildOutput, String> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture worker stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture worker stderr".to_string())?;
    let stdout_reader = thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.read_to_end(&mut data);
        data
    });
    let stderr_reader = thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stderr.read_to_end(&mut data);
        data
    });
    let started = Instant::now();
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| format!("failed waiting for Codex CLI: {error}"))?
        {
            Some(status) => break status,
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("worker timed out after {}ms", timeout.as_millis()));
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let mut stdout = stdout_reader
        .join()
        .map_err(|_| "worker stdout reader panicked".to_string())?;
    let mut stderr = stderr_reader
        .join()
        .map_err(|_| "worker stderr reader panicked".to_string())?;
    stdout.truncate(max_output_bytes);
    stderr.truncate(max_output_bytes);
    Ok((status.code(), stdout, stderr))
}

fn collaborative_prompt(worker: &Worker, worktree: &Path, namespace: &str) -> String {
    format!("You are a {} agent in a collaborative dual-mode swarm.\nPlatform: OpenAI Codex\nWorking Directory: {}\nShared Memory Namespace: {namespace}\n\nCOLLABORATION PROTOCOL:\n1. Search shared memory for context: ruflo memory search --query \"<relevant terms>\" --namespace {namespace}\n2. Complete your assigned task\n3. Store your results: ruflo memory store --key \"{}-result\" --value \"<your summary>\" --namespace {namespace}\n\nYOUR TASK:\n{}\n\nRemember: Other agents depend on your results in shared memory. Be concise and store actionable outputs.", worker.role.to_ascii_uppercase(), worktree.display(), worker.id, worker.prompt)
}

fn git<const N: usize>(project_root: &Path, arguments: [&str; N]) -> Result<String, String> {
    git_owned(
        project_root,
        arguments.into_iter().map(str::to_string).collect(),
    )
}

fn git_owned(project_root: &Path, arguments: Vec<String>) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(&arguments)
        .output()
        .map_err(|error| format!("failed to execute git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|argument| argument == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn parse_positive(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{name} must be a positive integer"))
}
fn parse_usize(value: &str, name: &str) -> Result<usize, String> {
    usize::try_from(parse_positive(value, name)?).map_err(|_| format!("{name} is too large"))
}
fn parse_bool(value: &str, name: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{name} must be true or false")),
    }
}
fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}
fn is_read_only_role(role: &str) -> bool {
    matches!(
        role.to_ascii_lowercase().as_str(),
        "reviewer"
            | "architect"
            | "analyzer"
            | "planner"
            | "scanner"
            | "auditor"
            | "security-scanner"
            | "security-analyst"
            | "code-analyzer"
            | "refactor-planner"
    )
}
fn is_sensitive_environment(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("RUFLO_")
        || upper.starts_with("CLAUDE_FLOW_POLICY_")
        || upper.split('_').any(|part| {
            matches!(
                part,
                "KEY" | "SECRET" | "TOKEN" | "PASSWORD" | "CREDENTIAL" | "CREDENTIALS"
            )
        })
}
fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn fail(message: String) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(1)
}
