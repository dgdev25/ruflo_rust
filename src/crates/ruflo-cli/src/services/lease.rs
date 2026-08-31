//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn acquire(workspace: &str, holder: &str, ttl_ms: u64) -> Result<Value, String> {
        // Lock around read-check-write so two callers can't both observe an
        // unleased workspace and both "win" it (lost-update → split-brain lease).
        let _guard = LockGuard::acquire("workspace-leases")
            .ok_or_else(|| "workspace-leases lock contention".to_string())?;
        let mut state = read_state("workspace-leases");
        let now = now_ms();
        let existing = state[workspace].clone();
        if !existing.is_null() {
            let expires = existing["expiresAt"].as_u64().unwrap_or(0);
            if expires > now && existing["holder"].as_str() != Some(holder) {
                return Err(format!("workspace `{workspace}` leased by {}", existing["holder"].as_str().unwrap_or("?")));
            }
        }
        let lease = json!({"holder": holder, "acquiredAt": now, "expiresAt": now + ttl_ms});
        state[workspace] = lease.clone();
        write_state("workspace-leases", &state);
        Ok(lease)
    }

    pub fn release(workspace: &str, holder: &str) -> bool {
        let mut state = read_state("workspace-leases");
        if state[workspace]["holder"].as_str() == Some(holder) {
            state[workspace] = Value::Null;
            write_state("workspace-leases", &state);
            return true;
        }
        false
    }
