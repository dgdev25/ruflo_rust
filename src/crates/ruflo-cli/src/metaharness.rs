//! Native V3 `metaharness` command — ADR-150 deep integration delegator.
//!
//! Source: `v3/@claude-flow/cli/src/commands/metaharness.ts`. Thin delegator
//! that spawns the plugin scripts at `plugins/ruflo-metaharness/scripts/`.
//! Graceful degradation (exit 0) when the plugin is absent. The flywheel
//! in-process branch needs ADR-322 transaction services (deferred).

use std::path::{Path, PathBuf};
use std::process::Command;

const SUBCOMMANDS: &[(&str, &str)] = &[
    ("score", "score.mjs"),
    ("genome", "genome.mjs"),
    ("mcp-scan", "mcp-scan.mjs"),
    ("threat-model", "threat-model.mjs"),
    ("oia-audit", "oia-audit.mjs"),
    ("audit-trend", "audit-trend.mjs"),
    ("audit-list", "audit-list.mjs"),
    ("similarity", "similarity.mjs"),
    ("drift-from-history", "drift-from-history.mjs"),
    ("mint", "mint.mjs"),
    ("redblue", "redblue.mjs"),
    ("learn", "learn.mjs"),
    ("gepa", "gepa.mjs"),
    ("evolve", "evolve.mjs"),
    ("bench", "bench.mjs"),
];

const OVERVIEW: &str = r####"ruflo metaharness <subcommand> [options]

Subcommands:
  score         5-dimension harness readiness scorecard
  genome        7-section categorical readiness report
  mcp-scan      static security scan of declared MCP surface
  threat-model  enterprise-grade threat model
  oia-audit     composite weekly audit (oia + threat + mcp) → memory
  audit-list    enumerate timestamped audit records
  audit-trend   diff two audit records (drift detection)
  similarity    ADR-152 — weighted similarity between two harness fingerprints
  drift-from-history  iter 53 — diff current state against most recent audit (1-command drift)
  mint          scaffold a custom harness (dry-run by default)
  redblue       adversarial red/blue LLM testing (init|run|patch|attack|report)
  evolve        Darwin candidate evolution
  bench         create or verify a stable benchmark suite
  flywheel      receipt loop: run | status | receipts | history | promote

Each subcommand accepts --format json|table and --help.

ADR-150 — runs as subprocess; graceful degradation if metaharness is not installed.
"####;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaCommand {
    pub subcommand: Option<String>,
    pub extra_args: Vec<String>,
}

pub fn run(_root: &Path, command: MetaCommand) -> u8 {
    let sub = command.subcommand.as_deref();
    if sub.is_none() || matches!(sub, Some("help") | Some("--help") | Some("-h")) {
        print!("{OVERVIEW}");
        return 0;
    }
    let sub = sub.unwrap();
    if sub == "flywheel" {
        eprintln!("[ERROR] metaharness flywheel requires the ADR-322 transaction service (not yet ported to native).");
        eprintln!("  Available operations: status | run | receipts | history | promote");
        return 1;
    }
    let script_name = match SUBCOMMANDS.iter().find(|(name, _)| *name == sub) {
        Some((_, script)) => *script,
        None => {
            eprintln!("[ERROR] Unknown subcommand: {sub}");
            eprintln!(
                "  Valid: {}",
                SUBCOMMANDS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return 2;
        }
    };
    let script_dir = locate_plugin_scripts(script_name);
    let Some(dir) = script_dir else {
        eprintln!("[WARN] metaharness: plugins/ruflo-metaharness/scripts/ not found. Install ruflo with `npm i ruflo` or run from the ruflo repo.");
        eprintln!("  (ADR-150 graceful degradation: this command is a thin delegator over the plugin; the plugin must be present.)");
        return 0; // feature-not-available, not a runtime failure
    };
    let script_path = dir.join(script_name);
    let rest = if command.extra_args.is_empty() {
        Vec::new()
    } else {
        command.extra_args.clone()
    };
    let status = Command::new("node").arg(&script_path).args(&rest).status();
    match status {
        Ok(s) => s.code().unwrap_or(1) as u8,
        Err(_) => {
            eprintln!("[ERROR] metaharness: failed to spawn node for {script_name}");
            1
        }
    }
}

/// Find `plugins/ruflo-metaharness/scripts/` containing `_harness.mjs` and the
/// required script. Pinned to cwd + the binary's install prefix only — NEVER
/// walks ancestor directories (security: an untrusted checkout could plant
/// attacker-controlled JS in an ancestor's plugins/ dir).
fn locate_plugin_scripts(required_script: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let candidates = vec![
        cwd.join("plugins/ruflo-metaharness/scripts"),
        cwd.join("node_modules/@claude-flow/cli/plugins/ruflo-metaharness/scripts"),
    ];
    for dir in &candidates {
        if dir.join("_harness.mjs").exists() && dir.join(required_script).exists() {
            return Some(dir.clone());
        }
    }
    None
}
