//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn commit_atomic(action: &str, data: Value) -> Result<Value, String> {
        // CAS: read champion → append receipt → verify chain.
        let receipt = crate::flywheel_ledger::append_receipt(action, &data);
        let (_, ok) = crate::flywheel_ledger::verify_ledger();
        if !ok {
            return Err("ledger verification failed after commit".into());
        }
        // Record in tx state.
        let mut state = read_state("flywheel-transactions");
        ensure_arr(&mut state, "transactions").push(receipt.clone());
        write_state("flywheel-transactions", &state);
        Ok(receipt)
    }
    pub fn history() -> Vec<Value> {
        read_state("flywheel-transactions")["transactions"].as_array().cloned().unwrap_or_default()
    }
