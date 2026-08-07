//! Shared native CLI entrypoint for thin Ruflo-compatible binaries.

mod command;

use std::ffi::OsString;
use std::process::ExitCode;

pub use command::ParsedCommand;

const VERSION: &str = concat!("ruflo v", env!("CARGO_PKG_VERSION"), "\n");
const HELP: &str = r#"
ruflo v3.34.0
Ruflo - AI Agent Orchestration Platform

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

const PLACEHOLDER_ERROR_EXIT: u8 = 2;

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
        Ok(ParsedCommand::McpStartPlaceholder {
            capability,
            wave,
            migration,
        }) => {
            let error = ruflo_types::RufloError::unsupported(ruflo_types::Capability::unsupported(
                capability, wave, migration,
            ));
            if let ruflo_types::RufloError::UnsupportedInWave { capability } = error {
                eprintln!(
                    "error: native MCP stdio dispatcher is not implemented yet (capability={}, wave={}, migration={})",
                    capability.name,
                    capability.wave,
                    capability.migration.as_deref().unwrap_or_default()
                );
            }
            ExitCode::from(PLACEHOLDER_ERROR_EXIT)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(PLACEHOLDER_ERROR_EXIT)
        }
    }
}
