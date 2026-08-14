//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn status() -> Value {
        read_state("agentic-bridge")
    }
    pub fn set_connected(version: &str) -> bool {
        write_state("agentic-bridge", &json!({"connected": true, "version": version, "connectedAt": now_ms()}));
        true
    }
