//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn record(model: &str, accuracy: f64) -> Value {
        let mut state = read_state("distill-oracle");
        let entry = json!({"model": model, "accuracy": accuracy, "at": now_ms()});
        ensure_arr(&mut state, "evaluations").push(entry.clone());
        write_state("distill-oracle", &state);
        entry
    }
    pub fn evaluations() -> Vec<Value> {
        read_state("distill-oracle")["evaluations"].as_array().cloned().unwrap_or_default()
    }
