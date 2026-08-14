//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn enqueue(task: Value) -> Value {
        // Lock so two concurrent enqueues produce two distinct queue entries
        // rather than one lost update.
        let _guard = match LockGuard::acquire("worker-queue") {
            Some(g) => g,
            None => return json!({}),
        };
        let mut state = read_state("worker-queue");
        let queue = ensure_arr(&mut state, "queue");
        let entry = json!({
            "id": unique_id("wq"),
            "task": task,
            "status": "queued",
            "enqueuedAt": now_ms(),
        });
        queue.push(entry.clone());
        write_state("worker-queue", &state);
        entry
    }

    pub fn dequeue() -> Option<Value> {
        // Lock so two concurrent dequeuers can't both pop the same head.
        let _guard = LockGuard::acquire("worker-queue")?;
        let mut state = read_state("worker-queue");
        let queue = ensure_arr(&mut state, "queue");
        if queue.is_empty() {
            return None;
        }
        let task = queue.remove(0);
        write_state("worker-queue", &state);
        Some(task)
    }

    pub fn list() -> Vec<Value> {
        read_state("worker-queue")["queue"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn length() -> usize {
        list().len()
    }
