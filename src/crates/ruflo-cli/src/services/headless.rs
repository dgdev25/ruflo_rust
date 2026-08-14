//! Auto-split from services.rs
use super::*;

    use super::*;
    use std::process::Command;
    use std::time::Duration;

    /// Launch a headless worker: spawn `claude -p <prompt>` (or codex) as a
    /// subprocess, capture stdout/stderr, enforce a timeout, and record the
    /// result in headless-workers state. Real behavioral parity with TS
    /// headless-worker-executor — no Node.
    ///
    /// Returns the worker result entry. `binary` is "claude" or "codex".
    pub fn execute(
        worker_type: &str,
        binary: &str,
        prompt: &str,
        timeout_ms: u64,
        extra_args: &[String],
    ) -> Value {
        let id = format!("headless-{worker_type}-{}", now_ms());
        let started = now_ms();
        let spend_permit = if binary == "claude" || binary == "codex" {
            match crate::spend::reserve("headless", worker_type, &root()) {
                Ok(p) => Some(p),
                Err(reason) => {
                    return json!({
                        "id": id, "type": worker_type, "binary": binary,
                        "status": "blocked", "error": reason,
                        "startedAt": started, "finishedAt": now_ms(),
                    });
                }
            }
        } else {
            None
        };
        let _spend_guard = crate::spend::PermitGuard::new(spend_permit);

        // Resolve binary; degrade cleanly if absent.
        if which(binary).is_none() {
            let entry = json!({
                "id": id, "type": worker_type, "binary": binary,
                "status": "unavailable", "error": format!("{binary} not on PATH"),
                "startedAt": started, "finishedAt": now_ms(),
            });
            record(&entry);
            return entry;
        }

        let mut cmd = Command::new(binary);
        // Use the correct invocation per binary (#11: codex needs different args).
        match binary {
            "codex" => {
                cmd.args(["exec", "--skip-git-repo-check", prompt]);
            }
            _ => {
                cmd.arg("-p").arg(prompt);
            }
        }
        cmd.env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GEMINI_API_KEY");
        for a in extra_args {
            cmd.arg(a);
        }
        cmd.stdin(std::process::Stdio::null())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                use std::io::Read;
                use std::sync::{Arc, Mutex};
                let stdout_h = child.stdout.take();
                let stderr_h = child.stderr.take();
                let stdout_buf = Arc::new(Mutex::new(Vec::new()));
                let stderr_buf = Arc::new(Mutex::new(Vec::new()));
                let so = stdout_buf.clone();
                let se = stderr_buf.clone();
                let so_t = std::thread::spawn(move || {
                    if let Some(mut h) = stdout_h {
                        let _ = h.read_to_end(&mut *so.lock().unwrap_or_else(|e| e.into_inner()));
                    }
                });
                let se_t = std::thread::spawn(move || {
                    if let Some(mut h) = stderr_h {
                        let _ = h.read_to_end(&mut *se.lock().unwrap_or_else(|e| e.into_inner()));
                    }
                });

                let timeout = Duration::from_millis(timeout_ms.max(1000));
                let start = std::time::Instant::now();
                let status = loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break Some(status),
                        Ok(None) => {
                            if start.elapsed() > timeout {
                                let _ = child.kill();
                                let _ = child.wait();
                                let _ = so_t.join();
                                let _ = se_t.join();
                                let entry = json!({
                                    "id": id, "type": worker_type, "binary": binary,
                                    "status": "timeout", "timeoutMs": timeout_ms,
                                    "startedAt": started, "finishedAt": now_ms(),
                                });
                                record(&entry);
                                return entry;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            let _ = so_t.join();
                            let _ = se_t.join();
                            let entry = json!({
                                "id": id, "type": worker_type, "binary": binary,
                                "status": "error", "error": e.to_string(),
                                "startedAt": started, "finishedAt": now_ms(),
                            });
                            record(&entry);
                            return entry;
                        }
                    }
                };
                let _ = so_t.join();
                let _ = se_t.join();
                let stdout_raw = stdout_buf.lock().unwrap_or_else(|e| e.into_inner());
                let stderr_raw = stderr_buf.lock().unwrap_or_else(|e| e.into_inner());
                let stdout_cap: String = String::from_utf8_lossy(&stdout_raw).chars().take(1_000_000).collect();
                let stderr_cap: String = String::from_utf8_lossy(&stderr_raw).chars().take(100_000).collect();
                let status = status.expect("loop only breaks with Some");
                let entry = json!({
                    "id": id, "type": worker_type, "binary": binary,
                    "status": if status.success() { "completed" } else { "failed" },
                    "exitCode": status.code(),
                    "stdout": stdout_cap, "stderr": stderr_cap,
                    "startedAt": started, "finishedAt": now_ms(),
                    "durationMs": now_ms().saturating_sub(started),
                });
                record(&entry);
                entry
            }
            Err(e) => {
                let entry = json!({
                    "id": id, "type": worker_type, "binary": binary,
                    "status": "spawn_failed", "error": e.to_string(),
                    "startedAt": started, "finishedAt": now_ms(),
                });
                record(&entry);
                entry
            }
        }
    }

    fn record(entry: &Value) {
        let mut state = read_state("headless-workers");
        ensure_arr(&mut state, "workers").push(entry.clone());
        write_state("headless-workers", &state);
    }

    fn which(bin: &str) -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).find_map(|dir| {
                let p = dir.join(bin);
                if p.is_file() { Some(p) } else { None }
            })
        })
    }

    pub fn launch(worker_type: &str, sandbox: &str) -> Value {
        let entry = json!({
            "id": format!("headless-{worker_type}-{}", now_ms()),
            "type": worker_type,
            "sandbox": sandbox,
            "status": "launched",
            "launchedAt": now_ms(),
        });
        let mut state = read_state("headless-workers");
        ensure_arr(&mut state, "workers").push(entry.clone());
        write_state("headless-workers", &state);
        entry
    }

    pub fn list() -> Vec<Value> {
        read_state("headless-workers")["workers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
