//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn acquire(pool_id: &str, max_concurrent: usize) -> Result<Value, String> {
        // Hold the lock for the full read-check-modify-write cycle so two
        // concurrent acquires can't both see capacity and both succeed.
        let _guard = LockGuard::acquire("bounded-pool")
            .ok_or_else(|| "bounded-pool lock contention".to_string())?;
        let mut state = read_state("bounded-pool");
        let key = pool_id;
        let active = state[key]["active"].as_array().cloned().unwrap_or_default();
        if active.len() >= max_concurrent {
            return Err(format!("pool `{pool_id}` at capacity ({max_concurrent})"));
        }
        let slot = json!({"id": unique_id("slot"), "acquiredAt": now_ms()});
        let mut arr = active;
        arr.push(slot.clone());
        state[key] = json!({"active": arr, "max": max_concurrent});
        write_state("bounded-pool", &state);
        Ok(slot)
    }

    pub fn release(pool_id: &str, slot_id: &str) -> bool {
        // Lock around read-modify-write so a concurrent acquire can't observe
        // a slot we're about to remove (and vice versa).
        let _guard = match LockGuard::acquire("bounded-pool") {
            Some(g) => g,
            None => return false,
        };
        let mut state = read_state("bounded-pool");
        if let Some(arr) = state[pool_id]["active"].as_array_mut() {
            let before = arr.len();
            arr.retain(|s| s["id"].as_str() != Some(slot_id));
            let changed = arr.len() < before;
            if changed {
                write_state("bounded-pool", &state);
            }
            return changed;
        }
        false
    }

    pub fn status(pool_id: &str) -> Value {
        let state = read_state("bounded-pool");
        state[pool_id].clone()
    }

    /// Run tasks with bounded concurrency. Each task is a (command, args) pair
    /// executed as a subprocess. Respects max_concurrent + timeout_ms per task.
    /// Returns the results array. This IS the live execution — ports the TS
    /// runBoundedPool executor natively.
    pub fn run_bounded(
        pool_id: &str,
        tasks: &[(String, Vec<String>)],
        max_concurrent: usize,
        timeout_ms: u64,
    ) -> Vec<Value> {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        let active = Arc::new(AtomicUsize::new(0));
        let max = max_concurrent.max(1);
        let results: Arc<std::sync::Mutex<Vec<Value>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut handles = Vec::new();

        for (i, (cmd, args)) in tasks.iter().enumerate() {
            while active.load(Ordering::Acquire) >= max {
                thread::sleep(std::time::Duration::from_millis(10));
            }
            active.fetch_add(1, Ordering::AcqRel);
            let _ = acquire(pool_id, max);

            let cmd = cmd.clone();
            let args = args.clone();
            let active = Arc::clone(&active);
            let pool_id = pool_id.to_string();
            let results = Arc::clone(&results);

            let handle = thread::spawn(move || {
                let start = now_ms();
                let status = std::process::Command::new(&cmd)
                    .args(&args)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn();
                let result = match status {
                    Ok(mut child) => {
                        // Watchdog timeout.
                        let timed_out = {
                            let start = std::time::Instant::now();
                            loop {
                                match child.try_wait() {
                                    Ok(Some(s)) => break json!({"task": i, "exit": s.code(), "ok": s.success()}),
                                    Ok(None) => {
                                        if start.elapsed().as_millis() as u64 > timeout_ms {
                                            let _ = child.kill();
                                            break json!({"task": i, "status": "timeout", "timeoutMs": timeout_ms});
                                        }
                                        thread::sleep(std::time::Duration::from_millis(50));
                                    }
                                    Err(e) => break json!({"task": i, "error": e.to_string()}),
                                }
                            }
                        };
                        timed_out
                    }
                    Err(e) => json!({"task": i, "error": format!("spawn {cmd}: {e}")}),
                };
                active.fetch_sub(1, Ordering::AcqRel);
                let _ = release(&pool_id, &format!("slot-{i}"));
                let mut r = results.lock().unwrap();
                r.push(result);
            });
            handles.push(handle);
        }
        for h in handles {
            let _ = h.join();
        }
        Arc::try_unwrap(results).ok().and_then(|m| m.into_inner().ok()).unwrap_or_default()
    }
