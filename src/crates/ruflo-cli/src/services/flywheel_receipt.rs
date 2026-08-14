//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn create(event: &str, payload: Value) -> Value {
        let mut state = read_state("flywheel-receipts");
        let receipt = json!({
            "id": unique_id("rcpt"),
            "event": event,
            "payload": payload,
            "createdAt": now_ms(),
        });
        ensure_arr(&mut state, "receipts").push(receipt.clone());
        write_state("flywheel-receipts", &state);
        receipt
    }
    pub fn list() -> Vec<Value> {
        read_state("flywheel-receipts")["receipts"].as_array().cloned().unwrap_or_default()
    }
