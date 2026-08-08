//! Shared native CLI entrypoint for thin Ruflo-compatible binaries.

mod announcements;
mod appliance;
mod appliance_advanced;
mod auth;
mod autopilot;
mod benchmark;
mod claims;
mod cleanup;
mod command;
mod completions;
mod compressor;
mod config_file;
mod deployment;
mod eject;
mod funnel;
mod funnel_command;
mod guidance;
mod issues;
mod lifecycle;
mod metaharness;
mod policy;
mod providers;
mod proxy;
mod settings;
mod spinner;
mod transfer_store;
mod update_cmd;
mod verify;
mod version;

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
const CLEANUP_HELP: &str = r#"
ruflo cleanup
Remove project artifacts created by claude-flow/ruflo

OPTIONS:
  -n, --dry-run             Show what would be removed without deleting (default behavior) [default: true]
  -f, --force               Actually delete the artifacts [default: false]
  -k, --keep-config         Preserve claude-flow.config.json and .claude/settings.json [default: false]

EXAMPLES:
  $ cleanup
    Show what would be removed (dry run)
  $ cleanup --force
    Remove all claude-flow artifacts
  $ cleanup --force --keep-config
    Remove artifacts but keep configuration files
"#;
const CONFIG_OVERVIEW: &str = r#"
Configuration Management

Usage: claude-flow config <subcommand> [options]

Subcommands:
  - init       - Initialize configuration
  - get        - Get configuration value
  - set        - Set configuration value
  - providers  - Manage AI providers
  - reset      - Reset to defaults
  - export     - Export configuration
  - import     - Import configuration
"#;
const TRANSPORT_OVERVIEW: &str = "\nAGNTCY/SLIM Transport\n============================================================\n\n+------------------------------ AGNTCY/SLIM Transport ------------------------------+\n| Local transport (in-process hooks routing) is the default.                        |\n|                                                                                   |\n| Subcommands:                                                                      |\n|                                                                                   |\n|   use <name>   Switch the active transport (currently: slim)                      |\n|                                                                                   |\n| See ADR-380 for setup: v3/docs/adr/ADR-380-agntcy-outshift-runtime-integration.md |\n+-----------------------------------------------------------------------------------+\n\n";
const TRANSPORT_HELP: &str = "\nruflo transport\nManage the active swarm/hive-mind coordination transport (ADR-380 §2)\n\nSUBCOMMANDS:\n  use             Switch the active swarm/hive-mind transport (e.g. slim) — ADR-380 §2\n\nEXAMPLES:\n  $ ruflo transport use slim\n    Switch coordination transport to SLIM\n";
const TRANSPORT_USE_HELP: &str = "\nruflo transport use\nSwitch the active swarm/hive-mind transport (e.g. slim) — ADR-380 §2\n\nEXAMPLES:\n  $ ruflo transport use slim\n    Switch coordination transport to SLIM (opt-in, degrades to local)\n";

pub fn run(argv: impl IntoIterator<Item = OsString>) -> ExitCode {
    match command::parse(argv) {
        Ok(ParsedCommand::Version) => {
            print!("{VERSION}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::VersionCommand {
            explain,
            require_catalog_gte,
        }) => ExitCode::from(version::run(version::VersionCommand {
            explain,
            require_catalog_gte,
        })),
        Ok(ParsedCommand::Completions { shell }) => ExitCode::from(completions::run(&shell)),
        Ok(ParsedCommand::CompletionsOverview) => ExitCode::from(completions::run_overview()),
        Ok(ParsedCommand::Doctor) => {
            let root = current_directory();
            let config = root.join(".claude-flow/config.json").is_file()
                || root.join(".claude-flow/config.yaml").is_file();
            let swarm = root.join(".swarm/state.json").is_file();
            let memory =
                root.join(".claude-flow/memory").is_dir() || root.join("data/memory").is_dir();
            let mcp_cfg = root.join(".claude-flow/mcp.json").is_file();
            let agents = root.join(".claude-flow/agents").is_dir();
            let sessions = root.join(".claude-flow/sessions").is_dir();
            let claims = root.join(".claude-flow/claims").is_dir();
            let hooks = root.join(".claude-flow/hooks.json").is_file()
                || root.join(".claude/settings.json").is_file();
            println!("Config File\t{}", if config { "pass" } else { "warn" });
            println!("Swarm State\t{}", if swarm { "pass" } else { "warn" });
            println!("Memory\t{}", if memory { "pass" } else { "warn" });
            println!("MCP Config\t{}", if mcp_cfg { "pass" } else { "warn" });
            println!("Agents\t{}", if agents { "pass" } else { "warn" });
            println!("Sessions\t{}", if sessions { "pass" } else { "warn" });
            println!("Claims\t{}", if claims { "pass" } else { "warn" });
            println!("Hooks/Settings\t{}", if hooks { "pass" } else { "warn" });
            println!("Native CLI\tpass");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::Start { topology, daemon }) => {
            match lifecycle::initialize_swarm(&current_directory(), &topology, 15, "development") {
                Ok(swarm) => {
                    if daemon {
                        let _ = std::fs::write(
                            current_directory().join(".claude-flow/daemon.pid"),
                            std::process::id().to_string(),
                        );
                    }
                    println!("RuFlo V3 is running!\nSwarm ID: {}\nTopology: {}\nMax Agents: 15\nMCP Server: stdio", swarm.id, swarm.topology);
                    ExitCode::SUCCESS
                }
                Err(error) => task_error(error),
            }
        }
        Ok(ParsedCommand::Progress) => {
            let metrics_path = current_directory().join(".claude-flow/metrics/v3-progress.json");
            if let Ok(raw) = std::fs::read_to_string(&metrics_path) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) {
                    let pct = data
                        .get("overall")
                        .or_else(|| data.get("progress"))
                        .and_then(|v| v.as_u64().or(v.as_f64().map(|f| f as u64)))
                        .unwrap_or(0);
                    println!("\nV3 Implementation Progress");
                    println!();
                    let filled = (pct as usize) * 30 / 100;
                    println!("[{}{}] {pct}%", "█".repeat(filled), "░".repeat(30 - filled));
                    if let Some(last) = data.get("lastUpdated").and_then(serde_json::Value::as_str)
                    {
                        println!("\nLast updated: {last}");
                    }
                    ExitCode::SUCCESS
                } else {
                    println!("Progress metrics file is malformed");
                    ExitCode::from(1)
                }
            } else {
                println!("\nV3 Implementation Progress");
                println!("\nNo metrics file found at .claude-flow/metrics/v3-progress.json");
                println!("Run 'ruflo progress sync' to calculate and persist progress.");
                ExitCode::SUCCESS
            }
        }
        Ok(ParsedCommand::CleanupHelp) => {
            print!("{CLEANUP_HELP}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::Cleanup { force, keep_config }) => {
            let result = cleanup::run(&current_directory(), force, keep_config);
            println!(
                "\n{}\n",
                if force {
                    "Claude Flow Cleanup"
                } else {
                    "Claude Flow Cleanup (dry run)"
                }
            );
            if result.artifacts.is_empty() {
                println!("No claude-flow artifacts found in the current directory.");
                return ExitCode::SUCCESS;
            }
            println!("Artifacts found:\n");
            let failed = result
                .failures
                .iter()
                .map(|(item, message)| (item.path, message.as_str()))
                .collect::<std::collections::BTreeMap<_, _>>();
            let mut skipped_count = 0;
            for item in &result.artifacts {
                let label = if item.kind == cleanup::ArtifactKind::Directory {
                    "dir "
                } else {
                    "file"
                };
                let size = cleanup::format_size(item.size);
                if item.skipped {
                    println!(
                        "  [skip] {label}  {}  ({size}) - {}",
                        item.path, item.description
                    );
                    skipped_count += 1;
                } else if !force {
                    println!(
                        "  [would remove] {label}  {}  ({size}) - {}",
                        item.path, item.description
                    );
                } else if let Some(message) = failed.get(item.path) {
                    println!("  [failed] {label}  {}  - {message}", item.path);
                } else {
                    println!(
                        "  [removed] {label}  {}  ({size}) - {}",
                        item.path, item.description
                    );
                }
            }
            println!("\nSummary:");
            if force {
                println!(
                    "  Removed {} artifact(s) totaling {}",
                    result.removed_count,
                    cleanup::format_size(result.removed_size)
                );
                if skipped_count > 0 {
                    println!("  Preserved {skipped_count} item(s) (--keep-config)");
                }
            } else {
                let actionable = result.artifacts.len() - skipped_count;
                println!(
                    "  Found {actionable} artifact(s) totaling {}",
                    cleanup::format_size(result.total_size)
                );
                if skipped_count > 0 {
                    println!("  {skipped_count} item(s) would be preserved (--keep-config)");
                }
                println!("\n  This was a dry run. Use --force to actually remove artifacts.");
            }
            println!();
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::TransportOverview) => {
            print!("{TRANSPORT_OVERVIEW}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::TransportHelp) => {
            print!("{TRANSPORT_HELP}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::TransportUseHelp) => {
            print!("{TRANSPORT_USE_HELP}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::TransportUse { name, quiet }) => transport_use(name.as_deref(), quiet),
        Ok(ParsedCommand::Deployment(command)) => {
            ExitCode::from(deployment::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Claims(command)) => {
            ExitCode::from(claims::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Advisor(command)) => ExitCode::from(funnel::run(command)),
        Ok(ParsedCommand::Announcements(command)) => ExitCode::from(announcements::run(command)),
        Ok(ParsedCommand::Spinner(command)) => ExitCode::from(spinner::run(command)),
        Ok(ParsedCommand::Settings(command)) => {
            ExitCode::from(settings::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Funnel(command)) => {
            ExitCode::from(funnel_command::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Eject(command)) => {
            ExitCode::from(eject::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Issues(command)) => {
            ExitCode::from(issues::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Benchmark(command)) => {
            ExitCode::from(benchmark::run(&current_directory(), command))
        }
        Ok(ParsedCommand::MetaHarness(command)) => {
            ExitCode::from(metaharness::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Verify(command)) => {
            ExitCode::from(verify::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Policy(command)) => {
            ExitCode::from(policy::run(&current_directory(), command))
        }
        Ok(ParsedCommand::UpdateCmd(command)) => {
            ExitCode::from(update_cmd::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Providers(command)) => {
            ExitCode::from(providers::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Auth(command)) => {
            ExitCode::from(auth::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Autopilot(command)) => {
            ExitCode::from(autopilot::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Proxy(command)) => {
            ExitCode::from(proxy::run(&current_directory(), command))
        }
        Ok(ParsedCommand::ApplianceAdvanced(command)) => {
            ExitCode::from(appliance_advanced::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Appliance(command)) => {
            ExitCode::from(appliance::run(&current_directory(), command))
        }
        Ok(ParsedCommand::TransferStore(command)) => {
            ExitCode::from(transfer_store::run(&current_directory(), command))
        }
        Ok(ParsedCommand::Guidance(command)) => {
            ExitCode::from(guidance::run(&current_directory(), command))
        }
        Ok(ParsedCommand::NativeOverview { name }) => {
            let root = current_directory();
            match name.as_str() {
                "neural" => {
                    println!("\nNeural Pattern Training");
                    println!("\nSubcommands:");
                    println!("  train     - Train neural patterns (WASM SIMD, MicroLoRA, Flash Attention)");
                    println!("  status    - Check training status");
                    println!("  patterns  - List learned patterns");
                    println!("  predict   - Make predictions");
                    println!("  optimize  - Optimize models");
                    let models = root.join(".claude-flow/neural/models");
                    if models.is_dir() {
                        let count = std::fs::read_dir(&models).map(|d| d.count()).unwrap_or(0);
                        println!("\nModels: {count} trained");
                    } else {
                        println!("\nNo models trained. Run 'ruflo neural train' to begin.");
                    }
                }
                "security" => {
                    println!("\nSecurity Scanning");
                    println!("\nSubcommands:");
                    println!("  scan       - Security scan");
                    println!("  cve        - CVE detection");
                    println!("  threats    - Threat modeling");
                    println!("  audit      - Security audit");
                    println!("  secrets    - Secrets scanning");
                }
                "performance" | "perf" => {
                    println!("\nPerformance Profiling");
                    println!("\nSubcommands:");
                    println!("  benchmark  - Run benchmarks");
                    println!("  profile    - Profile code");
                    println!("  metrics    - Show metrics");
                    println!("  optimize   - Optimize performance");
                    println!("  bottleneck - Find bottlenecks");
                }
                "hooks" => {
                    println!("\nSelf-Learning Hooks System");
                    println!("\nSubcommands:");
                    println!("  pre-edit    - Before file editing");
                    println!("  post-edit   - After file editing");
                    println!("  pre-command - Before command execution");
                    println!("  post-command- After command execution");
                    println!("  route       - Route task to agent");
                    println!("  intelligence- Neural intelligence commands");
                    let settings = root.join(".claude/settings.json");
                    if settings.is_file() {
                        println!("\nSettings: configured");
                    }
                }
                "workflow" => {
                    println!("\nWorkflow Execution");
                    println!("\nSubcommands:");
                    println!("  run      - Run a workflow");
                    println!("  validate - Validate workflow definition");
                    println!("  list     - List workflows");
                    println!("  status   - Workflow status");
                    let wf_dir = root.join(".claude-flow/workflows");
                    if wf_dir.is_dir() {
                        let count = std::fs::read_dir(&wf_dir).map(|d| d.count()).unwrap_or(0);
                        println!("\nWorkflows: {count} defined");
                    }
                }
                cmd => {
                    let overview = match cmd {
                        "embeddings" | "embed" => "\nSubcommands: init, generate, search, compare, collections, index, providers, chunk, normalize, hyperbolic, neural, models, cache, warmup, benchmark",
                        "verify" => "\nSubcommands: local, remote",
                        "analyze" | "an" => "\nSubcommands: diff, code, deps, ast, complexity, symbols, imports, boundaries, modules, dependencies, circular",
                        "route" => "\nSubcommands: task, list-agents, stats, feedback, reset, export, import, coverage",
                        "policy" => "\nSubcommands: status, init, migrate, evaluate, rule (add/list/remove), budget (set/show), approve, revoke, audit, verify",
                        "providers" => "\nSubcommands: list, configure, test, models, usage",
                        "plugins" => "\nSubcommands: list, search, install, uninstall, upgrade, toggle, info, create, rate",
                        "hive-mind" | "hive" => "\nSubcommands: init, spawn, status, task, join, leave, consensus, broadcast, memory, optimize-memory, shutdown",
                        "process" | "proc" | "ps" => "\nSubcommands: daemon, monitor, workers, signals, logs",
                        "daemon" => "\nSubcommands: start, stop, status, trigger, enable, budget (show/pause/resume)",
                        "update" => "\nSubcommands: check, all, history, rollback, clear-cache",
                        "guidance" | "guide" => "\nSubcommands: compile, retrieve, gates, status, optimize, ab-test",
                        "appliance" | "rvfa" => "\nSubcommands: build, inspect, verify, extract, run, sign, publish, update",
                        "appliance-advanced" => "\nSubcommands: sign, publish, update",
                        "transfer-store" => "\nSubcommands: list, search, download, publish, info",
                        "autopilot" | "ap" => "\nSubcommands: status, enable, disable, config, reset, log, learn, history, predict, check",
                        "gaia-bench" => "\nSubcommands: run",
                        "auth" => "\nSubcommands: status, login, logout",
                        "proxy" => "\nSubcommands: install, update, start, supervise, stop, status, logs, uninstall, config, sponsor, power-saver, training-share",
                        _ => "",
                    };
                    if overview.is_empty() {
                        println!("\n{cmd} — native surface active (full V3 parity pending)");
                    } else {
                        println!("\n{}{overview}", cmd);
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::MemoryStore {
            key,
            value,
            namespace,
            tags_json,
            provenance_type,
            upsert,
            path,
        }) => match open_memory_store(path.as_deref()).and_then(|store| {
            store.store(&ruflo_storage::MemoryStoreInput {
                key,
                namespace,
                content: value,
                memory_type: "semantic".into(),
                tags_json,
                provenance_type,
                upsert,
            })
        }) {
            Ok(entry) => {
                println!(
                    "Data stored successfully\n{}/{}",
                    entry.namespace, entry.key
                );
                ExitCode::SUCCESS
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::MemoryRetrieve {
            key,
            namespace,
            value_only,
            path,
        }) => match open_memory_store(path.as_deref())
            .and_then(|store| store.retrieve(&namespace, &key))
        {
            Ok(Some(entry)) if value_only => {
                print!("{}", entry.content);
                ExitCode::SUCCESS
            }
            Ok(Some(entry)) => {
                println!(
                    "Namespace: {}\nKey: {}\nValue:\n{}",
                    entry.namespace, entry.key, entry.content
                );
                ExitCode::SUCCESS
            }
            Ok(None) => {
                eprintln!("Key not found: {key}");
                ExitCode::from(1)
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::MemorySearch {
            query,
            namespace,
            limit,
            path,
        }) => match open_memory_store(path.as_deref())
            .and_then(|store| store.search_keyword(namespace.as_deref(), &query, limit))
        {
            Ok(entries) => {
                for entry in &entries {
                    println!("{}\t{}\t{}", entry.namespace, entry.key, entry.content);
                }
                println!("Found {} result(s) via keyword search", entries.len());
                ExitCode::SUCCESS
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::MemoryList {
            namespace,
            limit,
            path,
        }) => match open_memory_store(path.as_deref())
            .and_then(|store| store.list(namespace.as_deref(), limit))
        {
            Ok(entries) => {
                for entry in &entries {
                    println!("{}\t{}\t{}", entry.namespace, entry.key, entry.memory_type);
                }
                println!(
                    "{} memory entr{}",
                    entries.len(),
                    if entries.len() == 1 { "y" } else { "ies" }
                );
                ExitCode::SUCCESS
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::MemoryDelete {
            key,
            namespace,
            path,
        }) => match open_memory_store(path.as_deref())
            .and_then(|store| store.delete(&namespace, &key))
        {
            Ok(Some(_entry)) => {
                println!("Deleted \"{key}\" from namespace \"{namespace}\"");
                ExitCode::SUCCESS
            }
            Ok(None) => {
                eprintln!("Key not found: \"{key}\" in namespace \"{namespace}\"");
                ExitCode::from(1)
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::MemoryStats { path }) => match open_memory_store(path.as_deref())
            .and_then(|store| {
                let database_path = store.database_path().display().to_string();
                store.stats().map(|stats| (database_path, stats))
            }) {
            Ok((database_path, stats)) => {
                println!(
                    "Memory Statistics\nBackend: SQLite metadata projection (semantic RVF CLI wiring pending)\nTotal Entries: {}\nEntries With Vectors: {}\nContent Bytes: {}\nLocation: {}",
                    stats.total_entries,
                    stats.entries_with_vectors,
                    stats.total_content_bytes,
                    database_path,
                );
                ExitCode::SUCCESS
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::MemoryPurge {
            namespace,
            dry_run,
            force,
            path,
        }) => match open_memory_store(path.as_deref()).and_then(|store| {
            let count = store.count_namespace(&namespace)?;
            if dry_run {
                return Ok((count, None));
            }
            if !force {
                return Err(ruflo_types::RufloError::invalid_input(
                    "memory.purge.force",
                    "refusing non-interactive purge without --force; use --dry-run to preview",
                ));
            }
            store
                .purge_namespace(&namespace)
                .map(|deleted| (count, Some(deleted)))
        }) {
            Ok((count, None)) => {
                println!(
                    "Would permanently delete {count} entr{} from namespace \"{namespace}\" (dry run — nothing deleted)",
                    if count == 1 { "y" } else { "ies" }
                );
                ExitCode::SUCCESS
            }
            Ok((_count, Some(deleted))) => {
                println!(
                    "Purged {deleted} entr{} from namespace \"{namespace}\"",
                    if deleted == 1 { "y" } else { "ies" }
                );
                ExitCode::SUCCESS
            }
            Err(error) => ruflo_error(error),
        },
        Ok(ParsedCommand::ConfigInit { force, .. }) => {
            match config_file::create(&current_directory(), force) {
                Ok(path) => {
                    println!("\nConfiguration created: {}\n", path.display());
                    println!("Key defaults:");
                    println!("  swarm.topology     = hierarchical");
                    println!("  swarm.maxAgents    = 8");
                    println!("  memory.backend     = hybrid");
                    println!("  mcp.transportType  = stdio");
                    ExitCode::SUCCESS
                }
                Err(error) => task_error_with_exit(error, 1),
            }
        }
        Ok(ParsedCommand::ConfigOverview) => {
            print!("{CONFIG_OVERVIEW}");
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::ConfigHelp { subcommand }) => {
            print!("{}", config_help(subcommand.as_deref()));
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::ConfigGet { key, json }) => match config_file::load(&current_directory())
        {
            Ok(config) => {
                if let Some(key) = key {
                    match config_file::get(&config, &key) {
                        Some(value) if json => {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(
                                    &serde_json::json!({"key": key, "value": value})
                                )
                                .expect("JSON value")
                            );
                            ExitCode::SUCCESS
                        }
                        Some(value) => {
                            println!("{key} = {}", config_file::js_template(value));
                            ExitCode::SUCCESS
                        }
                        None => {
                            eprintln!("[ERROR] Configuration key not found: {key}");
                            ExitCode::from(1)
                        }
                    }
                } else {
                    let flattened = serde_json::Value::Object(config_file::flattened(&config));
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&flattened).expect("JSON value")
                        );
                    } else if let Some(entries) = flattened.as_object() {
                        println!("\nCurrent Configuration\n");
                        let rows = entries
                            .iter()
                            .map(|(key, value)| vec![key.clone(), display_table_value(value)])
                            .collect::<Vec<_>>();
                        println!(
                            "{}",
                            text_table(&[("Key", 25, false), ("Value", 30, false)], &rows)
                        );
                    }
                    ExitCode::SUCCESS
                }
            }
            Err(error) => task_error_with_exit(error, 1),
        },
        Ok(ParsedCommand::ConfigSet { key, value }) => {
            match config_file::set(&current_directory(), &key, config_file::parse_value(&value)) {
                Ok(_) => {
                    println!("Set {key} = {value}");
                    ExitCode::SUCCESS
                }
                Err(error) => task_error_with_exit(error, 1),
            }
        }
        Ok(ParsedCommand::ConfigProviders {
            add,
            remove,
            enable,
            disable,
            json,
        }) => {
            match config_file::providers(
                &current_directory(),
                add.as_deref(),
                remove.as_deref(),
                enable.as_deref(),
                disable.as_deref(),
            ) {
                Ok(result) => {
                    for message in &result.messages {
                        println!("{message}");
                    }
                    if let Some(failure) = result.failure {
                        eprintln!("[ERROR] {failure}");
                        return ExitCode::from(1);
                    }
                    let providers = result.providers;
                    if result.messages.is_empty() && json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&providers).expect("JSON value")
                        );
                    } else if result.messages.is_empty() {
                        let rows = providers.as_array().expect("providers are always an array");
                        println!("\nAI Providers\n");
                        let rows = rows
                            .iter()
                            .map(|provider| {
                                vec![
                                    provider
                                        .get("name")
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or("")
                                        .into(),
                                    provider
                                        .get("model")
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or("")
                                        .into(),
                                    provider
                                        .get("priority")
                                        .map(display_json_value)
                                        .unwrap_or_default(),
                                    provider
                                        .get("status")
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or_else(|| {
                                            if provider
                                                .get("enabled")
                                                .and_then(serde_json::Value::as_bool)
                                                .unwrap_or(true)
                                            {
                                                "Active"
                                            } else {
                                                "Disabled"
                                            }
                                        })
                                        .into(),
                                ]
                            })
                            .collect::<Vec<_>>();
                        println!(
                            "{}",
                            text_table(
                                &[
                                    ("Provider", 12, false),
                                    ("Model", 25, false),
                                    ("Priority", 10, true),
                                    ("Status", 10, false)
                                ],
                                &rows,
                            )
                        );
                        println!("\nUse --add, --remove, --enable, --disable to manage providers");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => task_error_with_exit(error, 1),
            }
        }
        Ok(ParsedCommand::ConfigReset { section, .. }) => {
            match config_file::reset(&current_directory(), section.as_deref()) {
                Ok(path) => {
                    println!("Configuration reset to defaults: {}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => task_error_with_exit(error, 1),
            }
        }
        Ok(ParsedCommand::ConfigExport { output, format: _ }) => {
            match config_file::export(&current_directory(), std::path::Path::new(&output)) {
                Ok(path) => {
                    println!("Configuration exported to: {}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => task_error_with_exit(error, 1),
            }
        }
        Ok(ParsedCommand::ConfigImport { file, merge }) => {
            match config_file::import(&current_directory(), std::path::Path::new(&file), merge) {
                Ok(_) => {
                    // config.ts:397 reports path.resolve(cwd, file) — lexical
                    // resolution, not a raw join (so `..` normalizes).
                    let path =
                        config_file::resolve(&current_directory(), std::path::Path::new(&file));
                    println!("Configuration imported from: {}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => task_error_with_exit(error, 1),
            }
        }
        Ok(ParsedCommand::MigrateStatus) => {
            let root = current_directory();
            let v2_config = root.join("claude-flow.config.json").is_file();
            let v3_config = root.join(".claude-flow").is_dir();
            let v2_memory = directory_has_entries(&root.join("data/memory"));
            let v2_sessions = directory_has_entries(&root.join("data/sessions"));
            let v2_agents = directory_has_entries(&root.join(".claude-flow/agents"));
            let v2_hooks = root.join("src/hooks").is_dir();
            let v2_workflows = directory_has_entries(&root.join(".claude-flow/workflows"));
            let needed =
                v2_config || v2_memory || v2_sessions || v2_agents || v2_hooks || v2_workflows;
            println!("Migration Status");
            println!(
                "Config\t{}\t{}",
                if v2_config {
                    "v2"
                } else if v3_config {
                    "v3"
                } else {
                    "missing"
                },
                if v2_config && !v3_config { "yes" } else { "no" }
            );
            for (label, found) in [
                ("Memory", v2_memory),
                ("Sessions", v2_sessions),
                ("Agents", v2_agents),
                ("Hooks", v2_hooks),
                ("Workflows", v2_workflows),
            ] {
                println!(
                    "{label}\t{}\t{}",
                    if found { "v2" } else { "missing" },
                    if found { "yes" } else { "no" }
                );
            }
            println!("Migration needed: {}", if needed { "yes" } else { "no" });
            ExitCode::SUCCESS
        }
        Ok(ParsedCommand::MigrateRun {
            target,
            dry_run,
            backup,
            force,
        }) => match migrate_v2_config(&current_directory(), &target, dry_run, backup, force) {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::from(1)
            }
        },
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
        Ok(ParsedCommand::Status) => {
            let root = current_directory();
            let config = root.join(".claude-flow/config.json").is_file()
                || root.join("claude-flow.config.json").is_file();
            let memory =
                root.join(".claude-flow/memory").is_dir() || root.join("data/memory").is_dir();
            let swarm = root.join(".swarm/state.json").is_file();
            let agents_dir = root.join(".claude-flow/agents");
            let sessions = root.join(".claude-flow/sessions");
            let mcp = root.join(".claude-flow/mcp.json").is_file();
            match lifecycle::status(&root) {
                Ok(status) => {
                    println!("RuFlo V3 [{}]", if swarm { "ACTIVE" } else { "STOPPED" });
                    println!();
                    println!(
                        "Configuration:\t{}",
                        if config { "initialized" } else { "not found" }
                    );
                    println!(
                        "Memory:\t\t{}",
                        if memory {
                            "hybrid backend"
                        } else {
                            "not initialized"
                        }
                    );
                    println!(
                        "MCP Server:\t{}",
                        if mcp { "configured" } else { "not configured" }
                    );
                    println!("Agents:\t\t{}", status.agents);
                    println!("Tasks:\t\t{}", status.tasks);
                    if agents_dir.is_dir() {
                        let count = std::fs::read_dir(&agents_dir)
                            .map(|d| d.count())
                            .unwrap_or(0);
                        println!("Agent defs:\t{count}");
                    }
                    if sessions.is_dir() {
                        let count = std::fs::read_dir(&sessions).map(|d| d.count()).unwrap_or(0);
                        println!("Sessions:\t{count}");
                    }
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
            }
        }
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
            eprintln!("[ERROR] {error}");
            ExitCode::from(1)
        }
    }
}

fn current_directory() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn config_help(subcommand: Option<&str>) -> &'static str {
    match subcommand {
        Some("init") => "\nruflo config init\nInitialize configuration\n\nOPTIONS:\n  -f, --force               Overwrite existing configuration [default: false]\n      --sparc               Initialize with SPARC methodology [default: false]\n      --v3                  Initialize V3 configuration [default: true]\n",
        Some("get") => "\nruflo config get\nGet configuration value\n\nOPTIONS:\n  -k, --key                 Configuration key (dot notation)\n\nEXAMPLES:\n  $ claude-flow config get swarm.topology\n    Get swarm topology\n  $ claude-flow config get -k memory.backend\n    Get memory backend\n",
        Some("set") => "\nruflo config set\nSet configuration value\n\nOPTIONS:\n  -k, --key                 Configuration key (required)\n  -v, --value               Configuration value (required)\n\nEXAMPLES:\n  $ claude-flow config set swarm.maxAgents 20\n    Set max agents\n  $ claude-flow config set -k memory.backend -v agentdb\n    Set memory backend\n",
        Some("providers") => "\nruflo config providers\nManage AI providers\n\nOPTIONS:\n  -a, --add                 Add provider\n  -r, --remove              Remove provider\n      --enable              Enable provider\n      --disable             Disable provider\n",
        Some("reset") => "\nruflo config reset\nReset configuration to defaults\n\nOPTIONS:\n  -f, --force               Skip confirmation [default: false]\n      --section             Reset specific section only\n",
        Some("export") => "\nruflo config export\nExport configuration\n\nOPTIONS:\n  -o, --output              Output file path\n  -f, --format              Export format (json, yaml) [default: json]\n",
        Some("import") => "\nruflo config import\nImport configuration\n\nOPTIONS:\n  -f, --file                Configuration file path (required)\n      --merge               Merge with existing configuration [default: false]\n",
        _ => "\nruflo config\nConfiguration management\n\nSUBCOMMANDS:\n  init            Initialize configuration\n  get             Get configuration value\n  set             Set configuration value\n  providers       Manage AI providers\n  reset           Reset configuration to defaults\n  export          Export configuration\n  import          Import configuration\n\nEXAMPLES:\n  $ claude-flow config init --v3\n    Initialize V3 config\n  $ claude-flow config get swarm.topology\n    Get config value\n  $ claude-flow config set swarm.maxAgents 20\n    Set config value\n",
    }
}

fn transport_use(name: Option<&str>, quiet: bool) -> ExitCode {
    let Some(name) = name.filter(|value| !value.is_empty()) else {
        eprintln!("[ERROR] Transport name required. Usage: ruflo transport use <name> (e.g. slim)");
        return ExitCode::from(1);
    };
    if name != "slim" {
        eprintln!("[ERROR] Unknown transport \"{name}\". Supported: slim.");
        if !quiet {
            eprintln!("[INFO] Local transport remains the default and needs no explicit \"use\".");
        }
        return ExitCode::from(1);
    }
    let endpoint = std::env::var("RUFLO_AGNTCY_SLIM_ENDPOINT").ok();
    match ruflo_runtime::activate_slim(endpoint.as_deref(), &ruflo_runtime::NoSlimTransportAdapter)
    {
        ruflo_runtime::TransportOutcome::SlimActivated { endpoint } => {
            println!("[OK] Active transport switched to: slim ({endpoint})");
        }
        ruflo_runtime::TransportOutcome::LocalFallback {
            reason,
            activation_error: true,
        } => {
            eprintln!("[ERROR] Failed to activate SLIM transport: {reason}");
            if !quiet {
                eprintln!("[INFO] Falling back to local transport.");
            }
        }
        ruflo_runtime::TransportOutcome::LocalFallback { .. } => {
            if !quiet {
                eprintln!("[INFO] AGNTCY/SLIM transport is not configured — see ADR-380 (v3/docs/adr/ADR-380-agntcy-outshift-runtime-integration.md) for setup. Falling back to local transport.");
                eprintln!("[INFO] Active transport remains: local (in-process hooks routing).");
            }
        }
    }
    ExitCode::SUCCESS
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

fn task_error_with_exit(error: std::io::Error, code: u8) -> ExitCode {
    eprintln!("[ERROR] {error}");
    ExitCode::from(code)
}

fn display_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "null".into(),
        other => other.to_string(),
    }
}

fn display_table_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .map(display_json_value)
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Object(_) => "[object Object]".into(),
        value => display_json_value(value),
    }
}

fn text_table(columns: &[(&str, usize, bool)], rows: &[Vec<String>]) -> String {
    // Char-based widths/truncation so multi-byte config keys/values don't split
    // on UTF-8 boundaries (matches JS string truncation, no panics).
    let char_len = |s: &str| s.chars().count();
    let widths = columns
        .iter()
        .enumerate()
        .map(|(index, (header, limit, _))| {
            let widest = rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| char_len(cell))
                .max()
                .unwrap_or(0);
            char_len(header).max(widest).min(*limit)
        })
        .collect::<Vec<_>>();
    let border = format!(
        "+{}+",
        widths
            .iter()
            .map(|width| "-".repeat(width + 2))
            .collect::<Vec<_>>()
            .join("+")
    );
    let render = |values: Vec<String>| {
        format!(
            "|{}|",
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let width = widths[index];
                    let value = if char_len(&value) > width {
                        let take = width.saturating_sub(3);
                        format!("{}...", value.chars().take(take).collect::<String>())
                    } else {
                        value
                    };
                    if columns[index].2 {
                        format!(" {:>width$} ", value, width = width)
                    } else {
                        format!(" {:<width$} ", value, width = width)
                    }
                })
                .collect::<Vec<_>>()
                .join("|")
        )
    };
    let mut lines = vec![
        border.clone(),
        render(
            columns
                .iter()
                .map(|(header, _, _)| (*header).into())
                .collect(),
        ),
        border.clone(),
    ];
    lines.extend(rows.iter().cloned().map(render));
    lines.push(border);
    lines.join("\n")
}

fn ruflo_error(error: ruflo_types::RufloError) -> ExitCode {
    eprintln!("error: {error}");
    ExitCode::from(ERROR_EXIT)
}

fn open_memory_store(
    path: Option<&str>,
) -> Result<ruflo_storage::SqliteMemoryStore, ruflo_types::RufloError> {
    let root = current_directory();
    let path = path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join(".swarm/memory.db"));
    ruflo_storage::SqliteMemoryStore::open(root, path)
}

fn directory_has_entries(path: &std::path::Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

fn migrate_v2_config(
    root: &std::path::Path,
    target: &str,
    dry_run: bool,
    backup: bool,
    force: bool,
) -> Result<String, String> {
    // Non-config targets are directory moves with optional backup.
    match target {
        "memory" => {
            return migrate_directory(
                root,
                "data/memory",
                ".claude-flow/memory",
                dry_run,
                backup,
                force,
            )
        }
        "sessions" => {
            return migrate_directory(
                root,
                "data/sessions",
                ".claude-flow/sessions",
                dry_run,
                backup,
                force,
            )
        }
        "agents" => {
            return migrate_directory(
                root,
                ".claude-flow/agents",
                "v3/agents",
                dry_run,
                backup,
                force,
            )
        }
        "hooks" => return migrate_directory(root, "src/hooks", "v3/hooks", dry_run, backup, force),
        "workflows" => {
            return migrate_directory(
                root,
                ".claude-flow/workflows",
                "data/workflows",
                dry_run,
                backup,
                force,
            )
        }
        _ => {}
    }
    let source = root.join("claude-flow.config.json");
    if !source.is_file() {
        return Err("no V2 configuration found at claude-flow.config.json".into());
    }
    let raw = std::fs::read_to_string(&source).map_err(|error| error.to_string())?;
    let mut config: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| format!("invalid V2 configuration: {error}"))?;
    let is_v2 = match config.get("version") {
        None => true,
        Some(serde_json::Value::String(value)) => value == "2",
        Some(serde_json::Value::Number(value)) => value.as_u64() == Some(2),
        _ => false,
    };
    if !is_v2 {
        return Err("configuration is not V2".into());
    }
    let destination = root.join(".claude-flow/config.json");
    if destination.exists() && !force {
        return Err("V3 config already exists; use --force to overwrite".into());
    }
    if dry_run {
        return Ok(format!(
            "Would migrate {target} configuration: {} -> {}",
            source.display(),
            destination.display()
        ));
    }
    if let Some(object) = config.as_object_mut() {
        object.insert("version".into(), serde_json::Value::String("3".into()));
        for (section, from, to) in [("swarm", "mode", "topology"), ("memory", "type", "backend")] {
            if let Some(section) = object
                .get_mut(section)
                .and_then(serde_json::Value::as_object_mut)
            {
                if !section.contains_key(to) {
                    if let Some(value) = section.remove(from) {
                        section.insert(to.into(), value);
                    }
                }
            }
        }
    }
    let parent = destination
        .parent()
        .ok_or("invalid V3 config destination")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if backup {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let backup_path = root.join(format!(
            ".claude-flow/backup/v2-{stamp}/claude-flow.config.json"
        ));
        std::fs::create_dir_all(backup_path.parent().ok_or("invalid backup path")?)
            .map_err(|error| error.to_string())?;
        std::fs::copy(&source, backup_path).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        &destination,
        serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        root.join(".claude-flow/migration-state.json"),
        r#"{"status":"complete","target":"config"}"#,
    )
    .map_err(|error| error.to_string())?;
    Ok(format!("Config migrated to {}", destination.display()))
}

/// Move a V2 source directory to its V3 destination (dry-run, backup, force).
fn migrate_directory(
    root: &std::path::Path,
    src_rel: &str,
    dst_rel: &str,
    dry_run: bool,
    backup: bool,
    force: bool,
) -> Result<String, String> {
    let source = root.join(src_rel);
    let dest = root.join(dst_rel);
    if !source.exists() {
        return Err(format!("source not found: {src_rel}"));
    }
    if dest.exists() && !force {
        return Err(format!(
            "destination already exists: {dst_rel} (use --force to overwrite)"
        ));
    }
    if dry_run {
        return Ok(format!("Would migrate {src_rel} -> {dst_rel}"));
    }
    if backup && dest.exists() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let backup_path = root.join(format!(".claude-flow/backup/v2-{stamp}/{src_rel}"));
        if let Some(parent) = backup_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&dest, &backup_path).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&source, &dest).map_err(|e| e.to_string())?;
    Ok(format!("Migrated {src_rel} -> {dst_rel}"))
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
