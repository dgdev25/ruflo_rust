//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn create_branch(name: &str) -> Value {
        let mut state = read_state("swarm-branches");
        state["branches"] = json!({}); let branches = state["branches"].as_object_mut().unwrap();
        branches.insert(name.into(), json!({"createdAt": now_ms(), "entries": {}}));
        write_state("swarm-branches", &state);
        json!({"branch": name, "created": true})
    }

    pub fn list_branches() -> Vec<String> {
        read_state("swarm-branches")["branches"]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
