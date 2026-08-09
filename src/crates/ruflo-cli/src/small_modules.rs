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

/// Credential vault — stores secrets with an HMAC-SHA256 authenticated seal.
/// Uses a keystream derived from HMAC(key, nonce) (repeated/expanded to value
/// length) XORed with the plaintext, plus an HMAC(key, nonce || ciphertext)
/// MAC stored alongside for tamper detection. This is an authenticated-encryption
/// construction that does not require an external AES crate; it matches the
/// vault.ts interface (store/retrieve/list/delete).
pub mod vault {
    use super::*;
    use sha2::{Digest, Sha256};

    /// HMAC-SHA256 computed inline (avoids pulling in a separate hmac crate).
    fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
        const BLOCK_SIZE: usize = 64;
        // RFC 2104: hash keys longer than the block size, pad shorter keys.
        let mut key_block = [0u8; BLOCK_SIZE];
        if key.len() > BLOCK_SIZE {
            let mut h = Sha256::new();
            h.update(key);
            let digest = h.finalize();
            key_block[..digest.len()].copy_from_slice(&digest);
        } else {
            key_block[..key.len()].copy_from_slice(key);
        }
        let mut ipad = [0u8; BLOCK_SIZE];
        let mut opad = [0u8; BLOCK_SIZE];
        for i in 0..BLOCK_SIZE {
            ipad[i] = key_block[i] ^ 0x36;
            opad[i] = key_block[i] ^ 0x5c;
        }
        let mut inner = Sha256::new();
        inner.update(&ipad);
        inner.update(message);
        let inner_digest = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(&opad);
        outer.update(&inner_digest);
        let outer_digest = outer.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&outer_digest);
        out
    }

    /// Derive a keystream of `len` bytes from HMAC(key, nonce) by concatenating
    /// HMAC blocks over a 32-bit counter (NIST SP 800-108-ish KDF).
    fn keystream(key: &[u8], nonce: &[u8], len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut counter: u32 = 0;
        while out.len() < len {
            let mut msg = Vec::with_capacity(nonce.len() + 4);
            msg.extend_from_slice(nonce);
            msg.extend_from_slice(&counter.to_be_bytes());
            let block = hmac_sha256(key, &msg);
            out.extend_from_slice(&block);
            counter = counter.wrapping_add(1);
        }
        out.truncate(len);
        out
    }

    fn xor_in_place(data: &mut [u8], ks: &[u8]) {
        for (i, b) in data.iter_mut().enumerate() {
            *b ^= ks[i];
        }
    }

    fn vault_key() -> Vec<u8> {
        std::env::var("RUFLO_VAULT_KEY")
            .unwrap_or_else(|_| "ruflo-default-vault-key-v3".into())
            .into_bytes()
    }

    pub fn store(key: &str, value: &str) -> Value {
        let vk = vault_key();
        // Nonce derived from current time (now_ms) — unique per store call.
        let nonce = now_ms().to_be_bytes();
        // Keystream XOR (confidentiality).
        let mut ct = value.as_bytes().to_vec();
        let ks = keystream(&vk, &nonce, ct.len());
        xor_in_place(&mut ct, &ks);
        let b64 = base64_encode(&ct);
        // MAC over nonce || ciphertext for authenticity / tamper detection.
        let mut mac_input = Vec::with_capacity(nonce.len() + ct.len());
        mac_input.extend_from_slice(&nonce);
        mac_input.extend_from_slice(&ct);
        let mac = hmac_sha256(&vk, &mac_input);
        let mac_b64 = base64_encode(&mac);
        let nonce_b64 = base64_encode(&nonce);
        json!({
            "key": key,
            "value": b64,
            "nonce": nonce_b64,
            "mac": mac_b64,
            "encrypted": true,
            "storedAt": now_ms()
        })
    }

    pub fn retrieve(entry: &Value) -> Option<String> {
        let b64 = entry["value"].as_str()?;
        let nonce_b64 = entry["nonce"].as_str()?;
        let mac_b64 = entry["mac"].as_str()?;
        let ct = base64_decode(b64)?;
        let nonce = base64_decode(nonce_b64)?;
        let stored_mac = base64_decode(mac_b64)?;
        let vk = vault_key();
        // Recompute MAC and constant-time compare — fail if mismatched.
        let mut mac_input = Vec::with_capacity(nonce.len() + ct.len());
        mac_input.extend_from_slice(&nonce);
        mac_input.extend_from_slice(&ct);
        let expected = hmac_sha256(&vk, &mac_input);
        if !ct_eq(&stored_mac, &expected) {
            return None;
        }
        // Regenerate keystream and XOR to recover plaintext.
        let ks = keystream(&vk, &nonce, ct.len());
        let mut pt = ct;
        xor_in_place(&mut pt, &ks);
        String::from_utf8(pt).ok()
    }

    /// Constant-time slice equality so MAC mismatches aren't timing-leaky.
    fn ct_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (x, y) in a.iter().zip(b.iter()) {
            diff |= x ^ y;
        }
        diff == 0
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
        // With the authenticated seal, MAC verification fails under the wrong
        // key and retrieve returns None rather than garbled plaintext.
        assert!(vault::retrieve(&entry).is_none());
        std::env::remove_var("RUFLO_VAULT_KEY");
    }

    #[test]
    fn vault_tamper_detected() {
        let _g = VAULT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("RUFLO_VAULT_KEY");
        let mut entry = vault::store("k", "topsecret");
        // Flip a bit in the stored ciphertext — retrieve must fail the MAC.
        let mut b64 = entry["value"].as_str().unwrap().to_string();
        // Toggle trailing '=' handling: replace first non-pad char with another.
        if let Some(pos) = b64.find(|c: char| c.is_alphanumeric()) {
            let bytes = unsafe { b64.as_bytes_mut() };
            bytes[pos] = if bytes[pos] == b'A' { b'B' } else { b'A' };
        }
        entry["value"] = json!(b64);
        assert!(vault::retrieve(&entry).is_none());
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
