//! Worker-free compatibility façade for the live `claude-flow-codex` CLI.
//!
//! Commands that would launch Codex or Claude workers are intentionally not
//! implemented here. They require the later native scheduler, policy,
//! worktree, cancellation, and durable-receipt contract.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

const VERSION: &str = "3.0.1\n";
const TEMPLATES: &str = r#"
Available Collaboration Templates:

feature - Feature Development Swarm
  Pipeline: architect → coder → tester → reviewer
  Platforms: Claude (architect, reviewer) + Codex (coder, tester)
  Usage: npx claude-flow-codex dual run --template feature --task "Add user auth"

security - Security Audit Swarm
  Pipeline: scanner → analyzer → fixer
  Platforms: Codex (scanner, fixer) + Claude (analyzer)
  Usage: npx claude-flow-codex dual run --template security --task "src/auth/"

refactor - Refactoring Swarm
  Pipeline: analyzer → planner → refactorer → validator
  Platforms: Claude (analyzer, planner) + Codex (refactorer, validator)
  Usage: npx claude-flow-codex dual run --template refactor --task "src/legacy/"

Custom configurations can be provided via --config <path.json>
"#;
const EMPTY_DUAL_RUN: &str = r#"═══════════════════════════════════════════════════════════════
  DUAL-MODE COLLABORATIVE EXECUTION
  Claude Code + Codex workers with shared memory
═══════════════════════════════════════════════════════════════

Please specify --template <name>, a [template] argument, --worker <spec> (repeatable), or --config <path>

Templates:
  feature  - Feature development (architect -> coder -> tester -> reviewer)
  security - Security audit (scanner -> analyzer -> fixer)
  refactor - Code refactoring (analyzer -> planner -> refactorer -> validator)

Custom workers:
  --worker "claude:architect:Design the API" --worker "codex:coder:Implement it"
"#;
const DUAL_RUN_HELP: &str = r#"Usage: claude-flow-codex dual run [options] [template]

Run a collaborative dual-mode swarm

Arguments:
  template               Pre-built template name (feature, security, refactor)
                         — positional alias for --template

Options:
  -t, --template <name>  Use a pre-built template (feature, security, refactor)
  -w, --worker <spec>    Worker spec "<platform>:<role>:<prompt>" (platform =
                         claude|codex). Repeatable. Workers chain sequentially
                         unless --parallel-workers. (default: [])
  --parallel-workers     Run --worker specs in parallel instead of chaining
                         them sequentially (default: false)
  -c, --config <path>    Path to collaboration config JSON
  --task <description>   Task description for the swarm
  --max-concurrent <n>   Maximum concurrent workers (default: "4")
  --timeout <ms>         Worker timeout in milliseconds (default: "300000")
  --namespace <name>     Shared memory namespace (default: "collaboration")
  -h, --help             display help for command
"#;
const DUAL_BANNER: &str = "═══════════════════════════════════════════════════════════════\n  DUAL-MODE COLLABORATIVE EXECUTION\n  Claude Code + Codex workers with shared memory\n═══════════════════════════════════════════════════════════════\n\n";

pub fn run(argv: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args = argv
        .into_iter()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    if borrowed.starts_with(&["dual", "run"]) {
        if let Some(error) = invalid_worker_spec(&args[2..]) {
            print!("{DUAL_BANNER}");
            eprintln!("Error: {error}");
            return ExitCode::from(1);
        }
    }

    if borrowed.starts_with(&["loop", "status"]) {
        return loop_status(&args[2..]);
    }
    if borrowed.starts_with(&["loop", "stop"]) {
        return loop_stop(&args[2..]);
    }
    if borrowed.starts_with(&["loop", "run"]) {
        return loop_run(&args[2..]);
    }
    if borrowed.starts_with(&["dual", "status"]) {
        return dual_status(&args[2..]);
    }

    match borrowed.as_slice() {
        ["--version"] | ["-v"] => {
            print!("{VERSION}");
            ExitCode::SUCCESS
        }
        ["dual", "templates"] => {
            print!("{TEMPLATES}");
            ExitCode::SUCCESS
        }
        ["dual", "run"] => {
            print!("{EMPTY_DUAL_RUN}");
            ExitCode::SUCCESS
        }
        ["dual", "run", "--help"] | ["dual", "run", "-h"] => {
            print!("{DUAL_RUN_HELP}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "error: unsupported native Codex compatibility invocation: {}",
                args.join(" ")
            );
            ExitCode::from(2)
        }
    }
}

fn invalid_worker_spec(args: &[String]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--worker" || argument == "-w" {
            let spec = args.get(index + 1)?;
            let mut parts = spec.splitn(3, ':');
            let platform = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
            let role = parts.next();
            let prompt = parts.next();
            if role.is_none() || prompt.is_none() {
                return Some(format!(
                    "Invalid --worker spec \"{spec}\". Expected \"<platform>:<role>:<prompt>\" (platform = claude|codex)."
                ));
            }
            if prompt.is_some_and(|value| value.trim().is_empty()) {
                return Some(format!(
                    "Invalid --worker spec \"{spec}\". Missing prompt after \"<platform>:<role>:\"."
                ));
            }
            if platform != "claude" && platform != "codex" {
                return Some(format!(
                    "Invalid platform \"{platform}\" in --worker spec \"{spec}\". Use \"claude\" or \"codex\"."
                ));
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    None
}

fn loop_status(args: &[String]) -> ExitCode {
    let options = LoopOptions::parse(args);
    let state_path = options.state_path();
    if !state_path.exists() {
        println!("No loop state found at {}", state_path.display());
        return ExitCode::SUCCESS;
    }

    match fs::read_to_string(&state_path) {
        Ok(state) if options.json => {
            print!("{}", state.trim_end());
            println!();
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!(
                "error: native loop state rendering is not yet available; use `loop status --json`"
            );
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!(
                "error: failed to read loop state {}: {error}",
                state_path.display()
            );
            ExitCode::from(2)
        }
    }
}

fn loop_stop(args: &[String]) -> ExitCode {
    let options = LoopOptions::parse(args);
    let stop_path = options.stop_path();
    if let Some(parent) = stop_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!(
                "error: failed to create loop state directory {}: {error}",
                parent.display()
            );
            return ExitCode::from(2);
        }
    }
    if let Err(error) = fs::write(&stop_path, "native stop requested\n") {
        eprintln!(
            "error: failed to write loop stop marker {}: {error}",
            stop_path.display()
        );
        return ExitCode::from(2);
    }
    println!("Stop requested: {}", stop_path.display());
    ExitCode::SUCCESS
}

fn loop_run(args: &[String]) -> ExitCode {
    let options = LoopOptions::parse(args);
    if !options.dry_run {
        eprintln!(
            "error: live native loop execution is unsupported; use --dry-run or the future policy-governed scheduler"
        );
        return ExitCode::from(2);
    }
    if options.command.is_none() && options.prompt.trim().is_empty() {
        eprintln!("loop run requires a prompt unless --command is provided");
        return ExitCode::from(1);
    }

    let state_path = options.state_path();
    let stop_path = options.stop_path();
    let complete_path = options.complete_path();
    let parent = match state_path.parent() {
        Some(parent) => parent,
        None => {
            eprintln!("error: loop state path has no parent");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = fs::create_dir_all(parent) {
        eprintln!(
            "error: failed to create loop state directory {}: {error}",
            parent.display()
        );
        return ExitCode::from(2);
    }
    if let Err(error) = fs::remove_file(&stop_path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "error: failed to reset loop stop marker {}: {error}",
                stop_path.display()
            );
            return ExitCode::from(2);
        }
    }

    let now = now_rfc3339_millis();
    let mode = if options.command.is_some() {
        "command"
    } else {
        "codex"
    };
    let mut state = json!({
        "name": options.name,
        "projectPath": options.project_path,
        "mode": mode,
        "status": "idle",
        "iteration": 0,
        "maxIterations": options.max_iterations,
        "intervalSeconds": options.interval_seconds,
        "startedAt": now,
        "updatedAt": now,
        "untilFile": complete_path,
        "prompt": options.prompt,
    });
    if let Some(command) = &options.command {
        state["command"] = json!(command);
    }
    let serialized = match serde_json::to_string_pretty(&state) {
        Ok(serialized) => serialized,
        Err(error) => {
            eprintln!("error: failed to serialize loop state: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = fs::write(&state_path, format!("{serialized}\n")) {
        eprintln!(
            "error: failed to write loop state {}: {error}",
            state_path.display()
        );
        return ExitCode::from(2);
    }

    print_loop_state(&options, mode, "idle", &complete_path);
    ExitCode::SUCCESS
}

fn dual_status(args: &[String]) -> ExitCode {
    let namespace = option_value(args, &["--namespace"]).unwrap_or_else(|| "collaboration".into());
    println!("\nDual-Mode Collaboration Status\n");
    println!("Memory Entries\n");

    let store = match ruflo_storage::SqliteMemoryStore::open_from_current_dir() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("error: failed to inspect shared memory: {error}");
            return ExitCode::from(2);
        }
    };
    let entries = match store.list(Some(&namespace), 20) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("error: failed to inspect shared memory: {error}");
            return ExitCode::from(2);
        }
    };
    if entries.is_empty() {
        println!("[WARN] No entries found");
        println!("[INFO] Store data: claude-flow memory store -k \"key\" --value \"data\"");
    } else {
        for entry in entries {
            println!("{}\t{}\t{}", entry.key, entry.namespace, entry.content);
        }
        println!("\n[INFO] Showing shared-memory entries for namespace {namespace}");
    }
    println!();
    ExitCode::SUCCESS
}

fn option_value(args: &[String], names: &[&str]) -> Option<String> {
    args.iter().enumerate().find_map(|(index, value)| {
        names
            .iter()
            .any(|name| value == name)
            .then(|| args.get(index + 1).cloned())
            .flatten()
    })
}

#[derive(Debug)]
struct LoopOptions {
    name: String,
    project_path: PathBuf,
    json: bool,
    dry_run: bool,
    command: Option<String>,
    prompt: String,
    interval_seconds: u64,
    max_iterations: u64,
}

impl LoopOptions {
    fn parse(args: &[String]) -> Self {
        let mut name = "default".to_string();
        let mut project_path = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut json = false;
        let mut dry_run = false;
        let mut command = None;
        let mut prompt = Vec::new();
        let mut interval_seconds = 270;
        let mut max_iterations = 10;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--name" | "-n" => {
                    if let Some(value) = args.get(index + 1) {
                        name = value.clone();
                        index += 1;
                    }
                }
                "--path" | "-p" => {
                    if let Some(value) = args.get(index + 1) {
                        project_path = absolute_path(Path::new(value));
                        index += 1;
                    }
                }
                "--json" => json = true,
                "--dry-run" => dry_run = true,
                "--command" => {
                    if let Some(value) = args.get(index + 1) {
                        command = Some(value.clone());
                        index += 1;
                    }
                }
                "--interval" | "-i" => {
                    if let Some(value) = args.get(index + 1) {
                        interval_seconds = value.parse().unwrap_or(270);
                        index += 1;
                    }
                }
                "--max-iterations" | "-m" => {
                    if let Some(value) = args.get(index + 1) {
                        max_iterations = value.parse().unwrap_or(10);
                        index += 1;
                    }
                }
                value if !value.starts_with('-') => prompt.push(value.to_string()),
                _ => {}
            }
            index += 1;
        }
        Self {
            name: normalize_loop_name(&name),
            project_path,
            json,
            dry_run,
            command,
            prompt: prompt.join(" "),
            interval_seconds,
            max_iterations,
        }
    }

    fn state_path(&self) -> PathBuf {
        self.project_path
            .join(".codex")
            .join("loop")
            .join(format!("{}.json", self.name))
    }

    fn stop_path(&self) -> PathBuf {
        self.project_path
            .join(".codex")
            .join("loop")
            .join(format!("{}.stop", self.name))
    }

    fn complete_path(&self) -> PathBuf {
        self.project_path
            .join(".codex")
            .join("loop")
            .join(format!("{}.complete", self.name))
    }
}

fn print_loop_state(options: &LoopOptions, mode: &str, status: &str, complete_path: &Path) {
    println!("Loop {}: {status}", options.name);
    println!("  mode:       {mode}");
    println!(
        "  iteration:  0/{}",
        if options.max_iterations == 0 {
            "unbounded".to_string()
        } else {
            options.max_iterations.to_string()
        }
    );
    println!("  interval:   {}s", options.interval_seconds);
    println!("  until file: {}", complete_path.display());
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn normalize_loop_name(name: &str) -> String {
    let normalized = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| match character {
            'a'..='z' | '0'..='9' | '_' | '.' | '-' => character,
            _ => '-',
        })
        .collect::<String>();
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        "default".to_string()
    } else {
        normalized.to_string()
    }
}

fn now_rfc3339_millis() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = elapsed.as_secs();
    let days = i64::try_from(total_seconds / 86_400).unwrap_or(i64::MAX);
    let time_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_date_from_days(days);
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        elapsed.subsec_millis()
    )
}

// Converts days since the Unix epoch to proleptic Gregorian Y-M-D. This is
// the public-domain civil-calendar algorithm adapted from Howard Hinnant.
fn civil_date_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_parameter = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_parameter + 2) / 5 + 1;
    let month = month_parameter + if month_parameter < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (
        year,
        u32::try_from(month).unwrap_or_default(),
        u32::try_from(day).unwrap_or_default(),
    )
}
