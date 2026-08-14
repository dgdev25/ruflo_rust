//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn validate(name: &str, checks: Vec<(&str, bool)>) -> Result<Value, String> {
        let failures: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
        let result = json!({
            "checkpoint": name,
            "passed": failures.is_empty(),
            "failures": failures,
            "at": now_ms(),
        });
        let mut state = read_state("checkpoints");
        ensure_arr(&mut state, "history").push(result.clone());
        write_state("checkpoints", &state);
        if failures.is_empty() {
            Ok(result)
        } else {
            Err(format!("checkpoint `{name}` failed: {}", failures.join(", ")))
        }
    }

    pub fn history() -> Vec<Value> {
        read_state("checkpoints")["history"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
