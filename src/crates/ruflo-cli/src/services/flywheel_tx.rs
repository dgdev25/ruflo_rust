//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn commit(action: &str, data: Value) -> Value {
        let mut state = read_state("flywheel-transactions");
        let tx = json!({"id": unique_id("tx"), "action": action, "data": data, "committedAt": now_ms()});
        ensure_arr(&mut state, "transactions").push(tx.clone());
        write_state("flywheel-transactions", &state);
        tx
    }
    pub fn history() -> Vec<Value> {
        read_state("flywheel-transactions")["transactions"].as_array().cloned().unwrap_or_default()
    }
