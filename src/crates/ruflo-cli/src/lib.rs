//! Shared native CLI entrypoint for thin Ruflo-compatible binaries.

mod command;
mod compressor;
mod lifecycle;

use std::ffi::OsString;
use std::process::ExitCode;

pub use command::ParsedCommand;

const VERSION: &str = concat!("ruflo v", env!("CARGO_PKG_VERSION"), "\n");
const HELP: &str = r#"
ruflo v3.34.0
RuFlo V3 - AI Agent Orchestration Platform

USAGE:
  ruflo <command> [subcommand] [options]

PRIMARY COMMANDS:
  init         Initialize RuFlo in the current directory
  start        Start the RuFlo orchestration system
  status       Show system status
  agent        Agent management commands
  swarm        Swarm coordination commands
  memory       Memory management commands
  task         Task management commands
  session      Session management commands
  mcp          MCP server management
  hooks        Self-learning hooks system for intelligent workflow automation

ADVANCED COMMANDS:
  neural       Neural pattern training, MoE, Flash Attention, pattern learning
  security     Security scanning, CVE detection, threat modeling, AI defense
  policy       Agentic policy engine — evaluate actions, manage rules/approvals, and verify the decision ledger (ADR-324)
  performance  Performance profiling, benchmarking, optimization, metrics
  embeddings   Vector embeddings, semantic search, similarity operations
  hive-mind    Queen-led consensus-based multi-agent coordination
  ruvector     RuVector PostgreSQL Bridge management
  guidance     Guidance Control Plane - compile, retrieve, enforce, and optimize guidance rules
  autopilot    Persistent swarm completion — keeps agents working until ALL tasks are done

UTILITY COMMANDS:
  config       Configuration management
  doctor       System diagnostics and health checks
  daemon       Manage background worker daemon (Node.js-based, auto-runs like shell helpers)
  completions  Generate shell completion scripts
  migrate      V2 to V3 migration tools
  workflow     Workflow execution and management

ANALYSIS COMMANDS:
  analyze      Code analysis, diff classification, graph boundaries, and change risk assessment
  route        Intelligent task-to-agent routing using Q-Learning
  progress     Check V3 implementation progress

MANAGEMENT COMMANDS:
  providers    Manage AI providers, models, and configurations
  plugins      Plugin management with IPFS-based decentralized registry
  deployment   Deployment management, environments, rollbacks
  claims       Claims-based authorization, permissions, and access control
  issues       Collaborative issue claims for human-agent workflows (ADR-016)
  update       Manage @claude-flow package updates (ADR-025)
  process      Background process management, daemon, and monitoring
  appliance    Self-contained RVFA appliance management (build, inspect, verify, extract, run)
  cleanup      Remove project artifacts created by claude-flow/ruflo

GLOBAL OPTIONS:
  -h, --help                Show help information
  -V, --version             Show version number
  -v, --verbose             Enable verbose output
  -Q, --quiet               Suppress non-essential output
  -c, --config              Path to configuration file
      --format              Output format (text, json, table)
      --no-color            Disable colored output
  -i, --interactive         Enable interactive mode

V3 FEATURES:
  - 15-agent hierarchical mesh coordination
  - AgentDB with HNSW indexing (150x-12,500x faster)
  - Flash Attention (2.49x-7.47x speedup)
  - Unified SwarmCoordinator engine
  - Event-sourced state management
  - Domain-Driven Design architecture

EXAMPLES:
  ruflo agent spawn -t coder              # Spawn a coder agent
  ruflo swarm init --v3-mode              # Initialize V3 swarm
  ruflo memory search -q "auth patterns"  # Semantic search
  ruflo mcp start                         # Start MCP server

Run "ruflo <command> --help" for command help

Created with ❤️ by ruv.io
"#;

const ERROR_EXIT: u8 = 2;

pub fn run(argv: impl IntoIterator<Item = OsString>) -> ExitCode {
    match command::parse(argv) {
        Ok(ParsedCommand::Version) => {
            print!("{VERSION}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::VersionCommand {
            explain,
            require_catalog_gte,
        }) => {
            let generation = 0_u64;
            if let Some(required) = require_catalog_gte {
                if generation >= required {
                    println!("OK (installed catalog is {generation})");
                    return ExitCode::SUCCESS;
                }
                eprintln!("Installed catalog generation {generation} is below required {required}");
                return ExitCode::from(1);
            }
            if explain {
                println!("Installed: ruflo@{}", env!("CARGO_PKG_VERSION"));
                println!("  (no catalog-manifest.json — plain semver, native dev checkout)");
            } else {
                println!("{}", env!("CARGO_PKG_VERSION"));
            }
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::Init) => match lifecycle::initialize(&current_directory()) {
            Ok(()) => {
                println!("RuFlo V3 initialized successfully!");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: failed to initialize RuFlo: {error}");
                ExitCode::from(ERROR_EXIT)
            }
        },
        Ok(ParsedCommand::Status) => match lifecycle::status(&current_directory()) {
            Ok(status) => {
                println!("RuFlo V3 [STOPPED]");
                println!("Agents: {}", status.agents);
                println!("Tasks: {}", status.tasks);
                ExitCode::SUCCESS
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("[ERROR] {error}");
                ExitCode::from(1)
            }
            Err(error) => {
                eprintln!("error: failed to read RuFlo status: {error}");
                ExitCode::from(ERROR_EXIT)
            }
        },
        Ok(ParsedCommand::SwarmInit {
            topology,
            max_agents,
            strategy,
        }) => match lifecycle::initialize_swarm(
            &current_directory(),
            &topology,
            max_agents,
            &strategy,
        ) {
            Ok(swarm) => {
                println!(
                    "Swarm {} initialized successfully ({}, max {} agents)",
                    swarm.id, swarm.topology, swarm.max_agents
                );
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::SwarmStatus) => match lifecycle::swarm_status(&current_directory()) {
            Ok(status) => match status.swarm {
                Some(swarm) => {
                    println!("Swarm {} [{}]", swarm.id, swarm.status);
                    println!(
                        "Topology: {} | Strategy: {}",
                        swarm.topology, swarm.strategy
                    );
                    println!(
                        "Agents: {}/{} active",
                        status.agents_active, status.agents_total
                    );
                    println!(
                        "Tasks: {}/{} completed, {} running",
                        status.tasks_completed, status.tasks_total, status.tasks_running
                    );
                    ExitCode::SUCCESS
                }
                None => {
                    println!("No active swarm");
                    ExitCode::SUCCESS
                }
            },
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::SwarmStart {
            objective,
            strategy,
        }) => {
            let project_root = current_directory();
            match lifecycle::start_swarm(&project_root, &objective, &strategy) {
                Ok(swarm) => {
                    println!("Starting swarm {}: {}", swarm.id, objective);
                    let workers = swarm_worker_plan(&swarm, &objective);
                    let code = ruflo_codex_cli::run_workers(&workers);
                    let succeeded = code == ExitCode::SUCCESS;
                    if let Err(error) = lifecycle::finish_swarm(&project_root, succeeded) {
                        eprintln!("error: failed to persist swarm completion: {error}");
                        return ExitCode::from(ERROR_EXIT);
                    }
                    code
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SwarmStop { swarm_id }) => {
            match lifecycle::stop_swarm(&current_directory(), &swarm_id) {
                Ok(swarm) => {
                    println!("Swarm {} stopped", swarm.id);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SwarmScale {
            swarm_id,
            target_agents,
            agent_type,
        }) => match lifecycle::scale_swarm(
            &current_directory(),
            &swarm_id,
            target_agents,
            agent_type.as_deref(),
        ) {
            Ok(result) => {
                println!(
                    "Swarm {} scaled to {} agents (delta {})",
                    result.swarm_id, result.target_agents, result.delta
                );
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::SwarmCoordinate { agents }) => {
            match lifecycle::coordinate_swarm(&current_directory(), agents) {
                Ok(swarm) => {
                    println!(
                        "V3 coordination initialized: {} agent slots ({})",
                        agents, swarm.id
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SwarmCompressMessage {
            message,
            message_file,
            budget_tokens,
            mode,
        }) => match match message {
            Some(message) => Ok(message),
            None => match message_file {
                Some(path) => read_project_message_file(&current_directory(), &path)
                    .map_err(|error| format!("Failed to read {path}: {error}")),
                None => Err("No message provided. Use --message or --message-file.".into()),
            },
        }
        .as_deref()
        .map_err(Clone::clone)
        .and_then(|message| compressor::compress_message(message, budget_tokens, &mode))
        {
            Ok(result) => {
                println!("{}", result.compressed);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(ERROR_EXIT)
            }
        },
        Ok(ParsedCommand::AgentSpawn { agent_type, name }) => {
            match lifecycle::spawn_agent(&current_directory(), &agent_type, &name) {
                Ok(agent) => {
                    println!(
                        "Agent {} spawned successfully ({})",
                        agent.id, agent.agent_type
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::from(ERROR_EXIT)
                }
            }
        }
        Ok(ParsedCommand::AgentList) => match lifecycle::list_agents(&current_directory()) {
            Ok(agents) => {
                for agent in agents {
                    println!("{}\t{}\t{}", agent.id, agent.agent_type, agent.status);
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(ERROR_EXIT)
            }
        },
        Ok(ParsedCommand::AgentStatus { agent_id }) => {
            match lifecycle::get_agent(&current_directory(), &agent_id) {
                Ok(agent) => {
                    println!("{}\t{}\t{}", agent.id, agent.agent_type, agent.status);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::AgentStop {
            agent_id,
            force,
            timeout_seconds,
        }) => match lifecycle::stop_agent(&current_directory(), &agent_id) {
            Ok(agent) => {
                let mode = if force { "forced" } else { "graceful" };
                println!(
                    "Agent {} stopped successfully ({mode}, {timeout_seconds}s)",
                    agent.id
                );
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::AgentMetrics { agent_id, period }) => {
            if let Some(agent_id) = agent_id {
                if let Err(error) = lifecycle::get_agent(&current_directory(), &agent_id) {
                    return task_error(error);
                }
            }
            match lifecycle::agent_metrics(&current_directory(), &period) {
                Ok(metrics) => {
                    println!(
                        "period={}\ttotal={}\tactive={}\tidle={}\tterminated={}",
                        metrics.period,
                        metrics.total_agents,
                        metrics.active_agents,
                        metrics.idle_agents,
                        metrics.terminated_agents
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::AgentPool {
            size,
            min,
            max,
            auto_scale,
        }) => {
            match lifecycle::configure_agent_pool(&current_directory(), size, min, max, auto_scale)
            {
                Ok(pool) => {
                    println!(
                        "pool\t{}\t{}\t{}\t{}",
                        pool.current_size, pool.min_size, pool.max_size, pool.auto_scale
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::AgentHealth {
            agent_id,
            detailed: _,
        }) => match lifecycle::agent_health(&current_directory(), agent_id.as_deref()) {
            Ok(agents) => {
                for (agent, health) in agents {
                    println!("{}\t{}\t{}", agent.id, agent.agent_type, health);
                }
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::AgentLogs {
            agent_id,
            tail,
            level,
            follow: _,
            since,
        }) => match lifecycle::agent_logs(
            &current_directory(),
            &agent_id,
            tail,
            &level,
            since.as_deref(),
        ) {
            Ok(entries) => {
                for entry in entries {
                    println!("{}\t{}\t{}", entry.timestamp_ms, entry.level, entry.message);
                }
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::TaskCreate {
            task_type,
            description,
            priority,
        }) => {
            match lifecycle::create_task(&current_directory(), &task_type, &description, &priority)
            {
                Ok(task) => {
                    println!("Task {} created successfully ({})", task.id, task.task_type);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::from(ERROR_EXIT)
                }
            }
        }
        Ok(ParsedCommand::TaskList) => match lifecycle::list_tasks(&current_directory()) {
            Ok(tasks) => {
                for task in tasks {
                    println!("{}\t{}\t{}", task.id, task.status, task.description);
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(ERROR_EXIT)
            }
        },
        Ok(ParsedCommand::TaskStatus { task_id }) => {
            match lifecycle::get_task(&current_directory(), &task_id) {
                Ok(task) => {
                    println!(
                        "{}\t{}\t{}\t{}",
                        task.id, task.status, task.priority, task.description
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::TaskCancel { task_id, reason }) => {
            match lifecycle::cancel_task(&current_directory(), &task_id, &reason) {
                Ok(task) => {
                    println!("Task {} cancelled", task.id);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::TaskAssign {
            task_id,
            agent_ids,
            unassign,
        }) => match lifecycle::assign_task(&current_directory(), &task_id, &agent_ids, unassign) {
            Ok(task) => {
                if unassign {
                    println!("Task {} unassigned", task.id);
                } else {
                    println!(
                        "Task {} assigned to {}",
                        task.id,
                        task.assigned_agent_ids.join(", ")
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::SessionSave { name, description }) => {
            match lifecycle::save_session(&current_directory(), &name, &description) {
                Ok(session) => {
                    println!("Session {} saved ({})", session.session_id, session.name);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SessionList) => match lifecycle::list_sessions(&current_directory()) {
            Ok(sessions) => {
                for session in sessions {
                    println!(
                        "{}\t{}\t{}",
                        session.session_id, session.status, session.name
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::SessionRestore { session_id }) => {
            match lifecycle::restore_session(&current_directory(), &session_id) {
                Ok(session) => {
                    println!("Session {} restored", session.session_id);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SessionDelete { session_id }) => {
            match lifecycle::delete_session(&current_directory(), &session_id) {
                Ok(()) => {
                    println!("Session {session_id} deleted");
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SessionExport { session_id, output }) => {
            let project_root = current_directory();
            let session_id = match session_id {
                Some(session_id) => session_id,
                None => match lifecycle::current_session(&project_root) {
                    Ok(session) => session.session_id,
                    Err(error) => return task_error(error),
                },
            };
            match lifecycle::export_session(
                &project_root,
                &session_id,
                std::path::Path::new(&output),
            ) {
                Ok(_) => {
                    println!("Session {session_id} exported to {output}");
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SessionImport { input, name }) => {
            match lifecycle::import_session(
                &current_directory(),
                std::path::Path::new(&input),
                name.as_deref(),
            ) {
                Ok(session) => {
                    println!("Session {} imported ({})", session.session_id, session.name);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::SessionCurrent) => match lifecycle::current_session(&current_directory())
        {
            Ok(session) => {
                println!(
                    "{}\t{}\t{}",
                    session.session_id, session.status, session.name
                );
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::TaskRetry {
            task_id,
            reset_state,
        }) => match lifecycle::retry_task(&current_directory(), &task_id, reset_state) {
            Ok(task) => {
                println!("Task {} retried ({})", task.id, task.status);
                ExitCode::SUCCESS
            }
            Err(error) => task_error(error),
        },
        Ok(ParsedCommand::McpStart) => {
            let config = match ruflo_config::EffectiveConfig::load() {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::from(ERROR_EXIT);
                }
            };

            let dispatcher = match ruflo_mcp::Dispatcher::from_config(config) {
                Ok(dispatcher) => dispatcher,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::from(ERROR_EXIT);
                }
            };

            match ruflo_mcp::serve_stdio(dispatcher) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::from(ERROR_EXIT)
                }
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(ERROR_EXIT)
        }
    }
}

fn current_directory() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn read_project_message_file(
    project_root: &std::path::Path,
    input: &str,
) -> Result<String, std::io::Error> {
    let path = project_root.join(input);
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "message file has no parent",
        )
    })?;
    if !std::fs::canonicalize(parent)?.starts_with(std::fs::canonicalize(project_root)?) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "message file must remain within project root",
        ));
    }
    std::fs::read_to_string(path)
}

fn task_error(error: std::io::Error) -> ExitCode {
    eprintln!("error: {error}");
    ExitCode::from(ERROR_EXIT)
}

fn swarm_worker_plan(swarm: &lifecycle::SwarmRecord, objective: &str) -> Vec<String> {
    let roles: &[&str] = match swarm.strategy.as_str() {
        "research" => &[
            "coordinator",
            "researcher",
            "researcher",
            "researcher",
            "researcher",
            "analyst",
            "analyst",
        ],
        "testing" => &["tester", "tester", "tester", "tester", "tester", "reviewer"],
        "optimization" => &["optimizer", "analyst", "analyst", "coder", "coder"],
        "maintenance" => &["coordinator", "coder", "coder", "researcher"],
        "analysis" => &["analyst", "analyst", "analyst", "reviewer"],
        "adaptive" => &["coordinator", "researcher", "coder", "coder", "coder"],
        "balanced" => &[
            "coordinator",
            "coder",
            "coder",
            "coder",
            "coder",
            "reviewer",
        ],
        "specialized" => &[
            "coordinator",
            "researcher",
            "architect",
            "coder",
            "coder",
            "tester",
            "reviewer",
        ],
        _ => &[
            "coordinator",
            "architect",
            "coder",
            "coder",
            "coder",
            "tester",
            "tester",
            "reviewer",
        ],
    };
    let mut args = vec![
        "--parallel-workers".into(),
        "--namespace".into(),
        format!("swarm-{}", swarm.id),
    ];
    for role in roles.iter().take(swarm.max_agents) {
        args.push("--worker".into());
        args.push(format!("codex:{role}:{objective}"));
    }
    args
}
