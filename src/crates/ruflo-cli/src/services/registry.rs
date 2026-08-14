//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn list_packages() -> Vec<Value> {
        read_state("registry")["packages"].as_array().cloned().unwrap_or_default()
    }
    pub fn register(name: &str, version: &str) -> Value {
        let mut state = read_state("registry");
        let entry = json!({"name": name, "version": version, "registeredAt": now_ms()});
        ensure_arr(&mut state, "packages").push(entry.clone());
        write_state("registry", &state);
        entry
    }
