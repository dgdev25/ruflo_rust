//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn get_stats() -> Value {
        read_state("ruvector-training")
    }
    pub fn record_session(model: &str, duration_ms: u64, patterns: usize) -> Value {
        let mut state = read_state("ruvector-training");
        let session = json!({"model": model, "durationMs": duration_ms, "patterns": patterns, "at": now_ms()});
        ensure_arr(&mut state, "sessions").push(session.clone());
        write_state("ruvector-training", &state);
        session
    }
