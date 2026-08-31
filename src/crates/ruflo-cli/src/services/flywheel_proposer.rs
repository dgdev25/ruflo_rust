//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn propose(candidate: &str, source: &str) -> Value {
        let mut state = read_state("flywheel-proposals");
        let prop = json!({"candidate": candidate, "source": source, "proposedAt": now_ms()});
        ensure_arr(&mut state, "proposals").push(prop.clone());
        write_state("flywheel-proposals", &state);
        prop
    }
    pub fn proposals() -> Vec<Value> {
        read_state("flywheel-proposals")["proposals"].as_array().cloned().unwrap_or_default()
    }
