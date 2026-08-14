//! Auto-split from services.rs
use super::*;

    use super::*;
    /// Tick: process one item from the worker queue via headless executor.
    pub fn tick(worker_type: &str) -> Value {
        let task = crate::services::worker_queue::dequeue();
        match task {
            Some(t) => {
                let task_desc = t["task"].to_string();
                let result = crate::services::headless::execute(
                    worker_type, "claude", &task_desc, 120_000, &[]
                );
                let entry = json!({
                    "workerType": worker_type, "task": t,
                    "result": result["status"], "at": now_ms(),
                });
                let mut state = read_state("worker-daemon");
                ensure_arr(&mut state, "processed").push(entry.clone());
                write_state("worker-daemon", &state);
                entry
            }
            None => json!({"status": "idle", "workerType": worker_type, "at": now_ms()}),
        }
    }
