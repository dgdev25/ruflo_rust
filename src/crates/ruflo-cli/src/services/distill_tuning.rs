//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn record_trial(params: Value, score: f64) -> Value {
        let mut state = read_state("distill-tuning");
        let trial = json!({"params": params, "score": score, "trialNum": state["trials"].as_array().map(|a| a.len()).unwrap_or(0), "at": now_ms()});
        ensure_arr(&mut state, "trials").push(trial.clone());
        write_state("distill-tuning", &state);
        trial
    }
    pub fn trials() -> Vec<Value> {
        read_state("distill-tuning")["trials"].as_array().cloned().unwrap_or_default()
    }
