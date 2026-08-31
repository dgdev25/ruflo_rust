//! Auto-split from services.rs
use super::*;

    use super::*;

    /// Create + run a container via docker subprocess. If docker isn't on
    /// PATH, records the intent + reports unavailable (not a silent stub).
    pub fn create(image: &str, cmd: &str) -> Value {
        let id = unique_id("container");
        let docker_available = std::process::Command::new("docker")
            .arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        let (status, container_id) = if docker_available {
            // Real docker run — detached container.
            let out = std::process::Command::new("docker")
                .args(["run", "-d", "--name", &id, image, "sh", "-c", cmd])
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    let cid = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    ("running".into(), cid)
                }
                Ok(o) => (format!("failed: {}", String::from_utf8_lossy(&o.stderr).trim()), String::new()),
                Err(e) => (format!("spawn: {e}"), String::new()),
            }
        } else {
            ("unavailable (docker not on PATH)".into(), String::new())
        };
        let entry = json!({
            "id": id, "containerId": container_id,
            "image": image, "command": cmd,
            "status": status, "createdAt": now_ms(),
        });
        let mut state = read_state("container-pool");
        ensure_arr(&mut state, "containers").push(entry.clone());
        write_state("container-pool", &state);
        entry
    }

    /// Stop + remove a container via docker.
    pub fn remove(container_id: &str) -> bool {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", container_id]).output();
        let mut state = read_state("container-pool");
        if let Some(arr) = state["containers"].as_array_mut() {
            arr.retain(|c| c["containerId"].as_str() != Some(container_id));
        }
        write_state("container-pool", &state);
        true
    }

    pub fn list() -> Vec<Value> {
        read_state("container-pool")["containers"]
            .as_array().cloned().unwrap_or_default()
    }
