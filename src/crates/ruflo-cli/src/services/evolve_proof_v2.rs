//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn accept(champion: &str, score: f64, threshold: f64) -> Value {
        let accepted = score >= threshold;
        let receipt = json!({
            "champion": champion, "score": score, "threshold": threshold,
            "accepted": accepted, "version": unique_id("proof"), "at": now_ms(),
        });
        let mut state = read_state("evolve-proof");
        ensure_arr(&mut state, "receipts").push(receipt.clone());
        write_state("evolve-proof", &state);
        receipt
    }
    pub fn receipts() -> Vec<Value> {
        read_state("evolve-proof")["receipts"].as_array().cloned().unwrap_or_default()
    }
