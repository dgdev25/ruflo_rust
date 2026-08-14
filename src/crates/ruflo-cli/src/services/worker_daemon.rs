//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn register_worker(worker_type: &str) -> Value {
        let mut state = read_state("worker-daemon");
        let workers = ensure_arr(&mut state, "workers");
        let entry = json!({
            "type": worker_type,
            "id": format!("daemon-{worker_type}-{}", now_ms()),
            "status": "registered",
            "registeredAt": now_ms(),
        });
        workers.push(entry.clone());
        write_state("worker-daemon", &state);
        entry
    }

    pub fn list_workers() -> Vec<Value> {
        read_state("worker-daemon")["workers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn unregister(worker_id: &str) -> bool {
        let mut state = read_state("worker-daemon");
        if let Some(workers) = state["workers"].as_array_mut() {
            let before = workers.len();
            workers.retain(|w| w["id"].as_str() != Some(worker_id));
            if workers.len() < before {
                write_state("worker-daemon", &state);
                return true;
            }
        }
        false
    }
