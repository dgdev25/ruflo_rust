//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn record(task: &str, weights: Vec<f64>) -> Value {
        let mut state = read_state("weight-eft");
        let entry = json!({"task": task, "weights": weights, "at": now_ms()});
        ensure_arr(&mut state, "records").push(entry.clone());
        write_state("weight-eft", &state);
        entry
    }
    pub fn records() -> Vec<Value> {
        read_state("weight-eft")["records"].as_array().cloned().unwrap_or_default()
    }
