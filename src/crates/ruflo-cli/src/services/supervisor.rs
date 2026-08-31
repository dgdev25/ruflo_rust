//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn record_check(status: &str, issues: Vec<String>) -> Value {
        let mut state = read_state("repo-supervisor");
        let entry = json!({
            "status": status,
            "issues": issues,
            "checkedAt": now_ms(),
        });
        ensure_arr(&mut state, "checks").push(entry.clone());
        write_state("repo-supervisor", &state);
        entry
    }

    pub fn latest() -> Option<Value> {
        let state = read_state("repo-supervisor");
        state["checks"].as_array()?.last().cloned()
    }
