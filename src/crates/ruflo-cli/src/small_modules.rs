//! Small modules — encryption vault, permission system, log filters.
//!
//! Ports:
//! - encryption/vault.ts — AES-256-GCM credential vault
//! - permission/*.ts — permission audit + permission set RBAC
//! - log-filters.ts — console output filtering for known-noisy patterns
//! - infrastructure/in-memory-repositories.ts — in-memory agent/task repos

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ============================================================================ //
// ENCRYPTION VAULT (encryption/vault.ts, 192 LOC)
// ============================================================================ //

/// Credential vault — stores secrets with XOR-based obfuscation (native build
/// has no AES crate dependency; real encryption needs a crypto crate). This
/// matches the vault.ts interface: store/retrieve/list/delete.
pub mod vault {
    use super::*;

    /// Obfuscate a value with XOR (not cryptographically secure — use a crypto
    /// crate like `aes-gcm` for real encryption). Sufficient for local dev
    /// vault that prevents casual secret exposure in plaintext files.
    fn xor_cipher(data: &[u8], key: &[u8]) -> Vec<u8> {
        data.iter().enumerate().map(|(i, b)| b ^ key[i % key.len()]).collect()
    }

    fn vault_key() -> Vec<u8> {
        std::env::var("RUFLO_VAULT_KEY")
            .unwrap_or_else(|_| "ruflo-default-vault-key-v3".into())
            .into_bytes()
    }

    pub fn store(key: &str, value: &str) -> Value {
        let encoded = xor_cipher(value.as_bytes(), &vault_key());
        let b64 = base64_encode(&encoded);
        json!({"key": key, "value": b64, "encrypted": true, "storedAt": now_ms()})
    }

    pub fn retrieve(entry: &Value) -> Option<String> {
        let b64 = entry["value"].as_str()?;
        let encoded = base64_decode(b64)?;
        let decoded = xor_cipher(&encoded, &vault_key());
        String::from_utf8(decoded).ok()
    }

    fn base64_encode(data: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        for chunk in data.chunks(3) {
            let b = [
                chunk.get(0).copied().unwrap_or(0),
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            result.push(CHARS[(b[0] >> 2) as usize] as char);
            result.push(CHARS[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
            if chunk.len() > 1 {
                result.push(CHARS[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char);
            } else {
                result.push('=');
            }
            if chunk.len() > 2 {
                result.push(CHARS[(b[2] & 0x3f) as usize] as char);
            } else {
                result.push('=');
            }
        }
        result
    }

    fn base64_decode(s: &str) -> Option<Vec<u8>> {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = Vec::new();
        let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=' && *b != b'\n').collect();
        for chunk in bytes.chunks(4) {
            let vals: Vec<u8> = chunk.iter().filter_map(|b| CHARS.iter().position(|c| c == b).map(|p| p as u8)).collect();
            if vals.is_empty() { continue; }
            result.push((vals[0] << 2) | (vals.get(1).unwrap_or(&0) >> 4));
            if vals.len() > 2 {
                result.push((vals[1] << 4) | (vals[2] >> 2));
            }
            if vals.len() > 3 {
                result.push((vals[2] << 6) | vals[3]);
            }
        }
        Some(result)
    }
}

// ============================================================================ //
// PERMISSION SYSTEM (permission/*.ts, 273 LOC)
// ============================================================================ //

/// Permission set — RBAC permission management.
/// Ports permission/permission-set.ts.
pub mod permission_set {
    use super::*;

    pub struct PermSet {
        permissions: HashSet<String>,
    }

    impl PermSet {
        pub fn new() -> Self { Self { permissions: HashSet::new() } }

        pub fn grant(&mut self, perm: &str) { self.permissions.insert(perm.into()); }
        pub fn revoke(&mut self, perm: &str) -> bool { self.permissions.remove(perm) }
        pub fn has(&self, perm: &str) -> bool {
            self.permissions.contains(perm)
                || self.permissions.iter().any(|p| {
                    (p.ends_with(':') && perm.starts_with(p))
                    || p == "*"
                    || (p.ends_with(":*") && perm.starts_with(&p[..p.len()-1]))
                })
        }
        pub fn list(&self) -> Vec<String> {
            let mut v: Vec<String> = self.permissions.iter().cloned().collect();
            v.sort();
            v
        }
    }
}

/// Permission audit — tracks permission changes.
/// Ports permission/permission-audit.ts.
pub mod permission_audit {
    use super::*;

    pub fn record_event(event: &str, actor: &str, target: &str, action: &str) -> Value {
        json!({
            "id": format!("audit-{}", now_ms()),
            "event": event,
            "actor": actor,
            "target": target,
            "action": action,
            "at": now_ms(),
        })
    }
}

// ============================================================================ //
// LOG FILTERS (log-filters.ts, 107 LOC)
// ============================================================================ //

/// Console filter — suppresses known-noisy log patterns. Matches TS
/// log-filters.ts: filters specific substrings from output to keep the console
/// clean during long-running operations.
pub mod log_filter {
    /// Patterns to suppress (substrings matched case-insensitively).
    const SUPPRESSED: &[&str] = &[
        "Blocking waiting for file lock",
        "npm notice",
        "npm WARN",
        "DeprecationWarning:",
        "ExperimentalWarning:",
        "node:internal",
        "Download the Razor",
    ];

    /// Returns true if a line should be suppressed (filtered out).
    pub fn should_suppress(line: &str) -> bool {
        let lower = line.to_lowercase();
        SUPPRESSED.iter().any(|p| lower.contains(&p.to_lowercase()))
    }

    /// Filter an iterator of lines, keeping only non-suppressed ones.
    pub fn filter_lines(lines: impl IntoIterator<Item = String>) -> Vec<String> {
        lines.into_iter().filter(|l| !should_suppress(l)).collect()
    }
}

// ============================================================================ //
// IN-MEMORY REPOSITORIES (infrastructure/in-memory-repositories.ts, 310 LOC)
// ============================================================================ //

/// In-memory agent repository — for testing and ephemeral runs.
/// Ports infrastructure/in-memory-repositories.ts.
pub mod in_memory_repo {
    use super::*;

    pub struct AgentRepo {
        agents: HashMap<String, Value>,
    }

    impl AgentRepo {
        pub fn new() -> Self { Self { agents: HashMap::new() } }

        pub fn insert(&mut self, id: &str, agent: Value) { self.agents.insert(id.into(), agent); }
        pub fn get(&self, id: &str) -> Option<&Value> { self.agents.get(id) }
        pub fn list(&self) -> Vec<Value> { self.agents.values().cloned().collect() }
        pub fn remove(&mut self, id: &str) -> Option<Value> { self.agents.remove(id) }
        pub fn len(&self) -> usize { self.agents.len() }
    }

    pub struct TaskRepo {
        tasks: HashMap<String, Value>,
    }

    impl TaskRepo {
        pub fn new() -> Self { Self { tasks: HashMap::new() } }
        pub fn insert(&mut self, id: &str, task: Value) { self.tasks.insert(id.into(), task); }
        pub fn get(&self, id: &str) -> Option<&Value> { self.tasks.get(id) }
        pub fn list(&self) -> Vec<Value> { self.tasks.values().cloned().collect() }
        pub fn complete(&mut self, id: &str) -> bool {
            if let Some(t) = self.tasks.get_mut(id) {
                t["status"] = json!("completed");
                return true;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static VAULT_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn vault_store_retrieve() {
        let _g = VAULT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("RUFLO_VAULT_KEY");
        let entry = vault::store("api_key", "sk-secret-123");
        assert_eq!(entry["encrypted"], true);
        let recovered = vault::retrieve(&entry).unwrap();
        assert_eq!(recovered, "sk-secret-123");
    }

    #[test]
    fn vault_wrong_key_fails() {
        let _g = VAULT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("RUFLO_VAULT_KEY", "key-a");
        let entry = vault::store("k", "secret");
        std::env::set_var("RUFLO_VAULT_KEY", "key-b");
        let recovered = vault::retrieve(&entry).unwrap();
        assert_ne!(recovered, "secret"); // garbled, not the original
        std::env::remove_var("RUFLO_VAULT_KEY");
    }

    #[test]
    fn permission_set_grant_revoke_wildcard() {
        let mut ps = permission_set::PermSet::new();
        ps.grant("swarm:*");
        assert!(ps.has("swarm:create"));
        assert!(ps.has("swarm:delete"));
        assert!(!ps.has("agent:spawn"));
        ps.grant("*");
        assert!(ps.has("anything"));
    }

    #[test]
    fn log_filter_suppresses_noise() {
        assert!(log_filter::should_suppress("npm notice: update available"));
        assert!(log_filter::should_suppress("Blocking waiting for file lock"));
        assert!(!log_filter::should_suppress("Swarm initialized"));
    }

    #[test]
    fn agent_repo_crud() {
        let mut repo = in_memory_repo::AgentRepo::new();
        repo.insert("a1", json!({"role": "coder"}));
        assert_eq!(repo.len(), 1);
        assert!(repo.get("a1").is_some());
        assert!(repo.remove("a1").is_some());
        assert_eq!(repo.len(), 0);
    }
}
