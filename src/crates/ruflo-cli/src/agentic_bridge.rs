//! agentic-flow bridge — connect a ruflo swarm to an agentic-flow fleet.
//!
//! Ports services/agentic-flow-bridge.ts behavioral parity. Detects the
//! `agentic-flow` binary (also `aqe`, `agflow` aliases), probes its version,
//! syncs topology, and relays messages — all via subprocess delegation.
//! Degrades with a documented reason when the fleet binary is absent.

use serde_json::{json, Value};
use std::process::Command;

/// Candidate binary names for the agentic-flow fleet CLI.
const BIN_NAMES: &[&str] = &["agentic-flow", "agflow", "aqe"];

/// Locate the fleet binary on PATH (None if absent).
pub fn detect_binary() -> Option<String> {
    for name in BIN_NAMES {
        if which(name).is_some() {
            return Some(name.to_string());
        }
    }
    None
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let p = dir.join(bin);
            if p.is_file() { Some(p) } else { None }
        })
    })
}

/// Probe the fleet's version + status. Returns a status object; available=
/// false + reason when the binary is absent.
pub fn status() -> Value {
    match detect_binary() {
        Some(bin) => {
            let version = Command::new(&bin).arg("--version").output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            json!({
                "available": true,
                "binary": bin,
                "version": version,
                "bridge": "native-subprocess",
            })
        }
        None => json!({
            "available": false,
            "reason": "agentic-flow binary not on PATH (looked for: agentic-flow, agflow, aqe)",
            "bridge": "none",
        }),
    }
}

/// Sync ruflo's swarm topology into a fleet init call. Returns the spawned
/// fleet's stdout (best-effort) or an error string.
pub fn sync_topology(topology: &str, max_agents: usize) -> Result<Value, String> {
    let bin = detect_binary()
        .ok_or_else(|| "agentic-flow not on PATH; cannot sync topology".to_string())?;
    let out = Command::new(&bin)
        .args(["fleet", "init", "--topology", topology,
               "--max-agents", &max_agents.to_string()])
        .output()
        .map_err(|e| format!("spawn {bin}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{bin} fleet init exited {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(json!({
        "synced": true, "binary": bin, "topology": topology,
        "maxAgents": max_agents, "output": stdout,
    }))
}

/// Relay a message into the fleet (e.g. a task handoff). Returns the fleet's
/// response.
pub fn relay(message: &str, target: &str) -> Result<Value, String> {
    let bin = detect_binary()
        .ok_or_else(|| "agentic-flow not on PATH; cannot relay".to_string())?;
    let out = Command::new(&bin)
        .args(["fleet", "broadcast", "--target", target, "--message", message])
        .output()
        .map_err(|e| format!("spawn {bin}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Ok(json!({
        "relayed": out.status.success(),
        "binary": bin, "target": target,
        "stdout": stdout.chars().take(2000).collect::<String>(),
        "stderr": stderr.chars().take(500).collect::<String>(),
    }))
}

/// Connect: probe + record. Used by `ruflo agentic-flow connect`.
pub fn connect() -> Value {
    let st = status();
    let dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let state_path = dir.join(".claude-flow/agentic-bridge.json");
    let _ = std::fs::create_dir_all(state_path.parent().unwrap());
    let mut state = st.clone();
    state["connectedAt"] = json!(now_ms());
    let tmp = state_path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&state).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &state_path);
    }
    state
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_availability() {
        let s = status();
        // Either available (binary present) or unavailable — both valid.
        assert!(s["available"].is_boolean());
    }

    #[test]
    fn sync_topology_errors_without_binary() {
        // Force-detect by checking the real result of detect_binary.
        if detect_binary().is_none() {
            let r = sync_topology("mesh", 8);
            assert!(r.is_err());
        }
        // If binary IS present, the call may succeed or fail on its args —
        // we don't assert that path (depends on the external CLI).
    }

    #[test]
    fn connect_records_state() {
        let _ = connect();
        // State file should now exist.
        let dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let p = dir.join(".claude-flow/agentic-bridge.json");
        assert!(p.exists(), "connect should persist state");
    }
}
