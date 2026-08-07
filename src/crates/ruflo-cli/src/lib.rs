//! Shared native CLI entrypoint for thin Ruflo-compatible binaries.

mod command;
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
