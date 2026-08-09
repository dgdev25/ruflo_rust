//! Full init wizard — ports init/ generators from TS.
//!
//! Creates the complete Claude/Codex/MCP/hooks/agents/memory project layout:
//! - .claude/settings.json (hook configurations)
//! - .claude/CLAUDE.md (project instructions)
//! - .mcp.json (MCP server config for Claude Code integration)
//! - .claude/settings.local.json (local settings)
//! - Statusline configuration
//! - Memory package scaffold

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

/// Run the full init wizard: create directory structure + all config files.
/// Returns a list of created file paths.
pub fn run_full_init(root: &Path) -> Vec<String> {
    let mut created = Vec::new();

    // Directory structure.
    for dir in [
        ".claude-flow/data", ".claude-flow/logs", ".claude-flow/sessions",
        ".claude-flow/agents", ".claude-flow/workflows", ".claude-flow/services",
        ".claude-flow/memory",
        ".claude-flow/neural/models", ".claude-flow/neural/patterns",
        ".claude-flow/backups", ".claude-flow/worktrees",
        ".claude", ".claude/skills", ".claude/agents", ".claude/commands",
        ".swarm/agents", ".swarm/tasks", ".swarm/memory", ".swarm/logs",
        ".agents",
    ] {
        let _ = fs::create_dir_all(root.join(dir));
    }

    // 1. .claude-flow/config.yaml
    let cfg_path = root.join(".claude-flow/config.yaml");
    if !cfg_path.exists() {
        let _ = fs::write(&cfg_path, CONFIG_YAML);
        created.push(".claude-flow/config.yaml".into());
    }

    // 2. .claude/settings.json — hooks + statusLine
    let settings_path = root.join(".claude/settings.json");
    if !settings_path.exists() {
        let settings = generate_settings();
        let _ = fs::write(&settings_path, serde_json::to_vec_pretty(&settings).unwrap_or_default());
        created.push(".claude/settings.json".into());
    }

    // 3. .claude/CLAUDE.md — project instructions
    let claudemd = root.join(".claude/CLAUDE.md");
    if !claudemd.exists() {
        let _ = fs::write(&claudemd, CLAUDE_MD_TEMPLATE);
        created.push(".claude/CLAUDE.md".into());
    }

    // 4. .mcp.json — MCP server config
    let mcp_path = root.join(".mcp.json");
    if !mcp_path.exists() {
        let mcp = generate_mcp_config(root);
        let _ = fs::write(&mcp_path, serde_json::to_vec_pretty(&mcp).unwrap_or_default());
        created.push(".mcp.json".into());
    }

    // 5. .agents/config.toml
    let agents_cfg = root.join(".agents/config.toml");
    if !agents_cfg.exists() {
        let _ = fs::write(&agents_cfg, "[swarm.automation]\nenabled = false\n");
        created.push(".agents/config.toml".into());
    }

    // 6. .claude/settings.local.json (local overrides, gitignored)
    let local_settings = root.join(".claude/settings.local.json");
    if !local_settings.exists() {
        let _ = fs::write(&local_settings, "{\n  \"env\": {}\n}\n");
        created.push(".claude/settings.local.json".into());
    }

    // 7. Memory package scaffold
    let mem_file = root.join(".claude-flow/memory/package.json");
    if !mem_file.exists() {
        let _ = fs::write(&mem_file, serde_json::to_vec_pretty(&json!({
            "name": "ruflo-memory",
            "version": "1.0.0",
            "description": "Persistent project memory for Ruflo V3",
            "backend": "sqlite",
            "createdAt:": now_iso(),
        })).unwrap_or_default());
        created.push(".claude-flow/memory/package.json".into());
    }

    // 8. .claude-flow/agents/default.json — default agent registry
    let agents_file = root.join(".claude-flow/agents/default.json");
    if !agents_file.exists() {
        let _ = fs::write(&agents_file, serde_json::to_vec_pretty(&json!({
            "agents": [],
            "version": 1,
        })).unwrap_or_default());
        created.push(".claude-flow/agents/default.json".into());
    }

    created
}

fn generate_settings() -> Value {
    json!({
        "hooks": {
            "PreToolUse": [
                {"matcher": "Edit|Write", "hooks": [{"type": "command", "command": "ruflo hooks pre-edit --file \"$CLAUDE_FILE_PATH\""}]}
            ],
            "PostToolUse": [
                {"matcher": "Edit|Write", "hooks": [{"type": "command", "command": "ruflo hooks post-edit --file \"$CLAUDE_FILE_PATH\""}]},
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "ruflo hooks post-command --command \"$CLAUDE_COMMAND\""}]}
            ],
            "UserPromptSubmit": [
                {"hooks": [{"type": "command", "command": "ruflo hooks route -t \"$CLAUDE_PROMPT\""}]}
            ]
        },
        "statusLine": {
            "type": "command",
            "command": "ruflo hooks statusline"
        }
    })
}

fn generate_mcp_config(root: &Path) -> Value {
    // Use "ruflo" (assumed on PATH) instead of an absolute exe path that
    // breaks if the binary is moved. Fix for codesec finding: brittle path.
    json!({
        "mcpServers": {
            "ruflo": {
                "command": "ruflo",
                "args": ["mcp", "start"],
                "cwd": root.display().to_string()
            }
        }
    })
}

const CONFIG_YAML: &str = r#"# Ruflo V3 Configuration
swarm:
  topology: hierarchical-mesh
  maxAgents: 15
  consensus: byzantine
memory:
  backend: hybrid
hooks:
  enabled: true
  learning: true
neural:
  wasm: true
  flash: true
  contrastive: true
security:
  scan: standard
  defend: true
"#;

const CLAUDE_MD_TEMPLATE: &str = r#"# Project Instructions

This project is managed by Ruflo V3 — AI Agent Orchestration Platform.

## Quick Start

- `ruflo status` — Check system status
- `ruflo swarm init` — Initialize a swarm
- `ruflo swarm start --objective "..."` — Start agents working
- `ruflo memory store --key <k> --value <v>` — Store a decision
- `ruflo memory search -q "..."` — Recall past decisions
- `ruflo security scan -t .` — Security scan

## Architecture

- **Swarm**: Hierarchical-mesh coordination (queen + peer agents)
- **Memory**: SQLite + semantic vector search
- **Hooks**: Self-learning event recording
- **Neural**: Pattern training (native vectorizer; WASM SIMD in Node build)

## Conventions

- Use `ruflo` (not `npx claude-flow`) for all operations.
- Record important decisions with `ruflo memory store`.
- Check security with `ruflo security scan` before commits.
- Route tasks intelligently with `ruflo hooks route`.

Created by Ruflo V3 init.
"#;

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn full_init_creates_all_files() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let created = run_full_init(dir.path());
        // Should create at least 8 files.
        assert!(created.len() >= 8, "expected >=8 files, got {}", created.len());
        // Key files exist.
        assert!(dir.path().join(".claude/settings.json").is_file());
        assert!(dir.path().join(".claude/CLAUDE.md").is_file());
        assert!(dir.path().join(".mcp.json").is_file());
        assert!(dir.path().join(".claude-flow/config.yaml").is_file());
        assert!(dir.path().join(".agents/config.toml").is_file());
    }

    #[test]
    fn init_is_idempotent() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let first = run_full_init(dir.path());
        let second = run_full_init(dir.path());
        // Second run should not create any new files (all already exist).
        assert!(second.is_empty(), "expected 0 new files on re-init, got {:?}", second);
        assert!(!first.is_empty());
    }

    #[test]
    fn settings_has_hooks() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        run_full_init(dir.path());
        let raw = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        let settings: Value = serde_json::from_str(&raw).unwrap();
        assert!(settings["hooks"]["PreToolUse"].is_array());
        assert!(settings["hooks"]["PostToolUse"].is_array());
        assert!(settings["statusLine"]["command"].is_string());
    }
}
