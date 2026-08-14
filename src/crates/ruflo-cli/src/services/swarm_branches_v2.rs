//! Auto-split from services.rs
use super::*;

    use super::*;

    /// Create a memory branch by copying the parent RVF store. This is a full
    /// file copy (not the 162-byte agenticow COW), but achieves the same
    /// service-level behavior: isolated per-agent memory.
    pub fn create_branch(name: &str, parent: &str) -> Result<Value, String> {
        let parent_path = std::path::PathBuf::from(parent);
        let branch_dir = root().join(".claude-flow/branches");
        let _ = fs::create_dir_all(&branch_dir);
        let safe_name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect(); let branch_path = branch_dir.join(format!("{safe_name}.rvf"));
        if parent_path.exists() {
            fs::copy(&parent_path, &branch_path).map_err(|e| e.to_string())?;
        } else {
            // No parent — start with an empty store (created on first ingest).
        }
        let entry = json!({
            "name": name, "parent": parent,
            "path": branch_path.display().to_string(),
            "createdAt": now_ms(),
        });
        let mut state = read_state("swarm-branches");
        if state["branches"].is_null() { state["branches"] = json!([]); }
        if let Some(arr) = state["branches"].as_array_mut() {
            arr.push(entry.clone());
        }
        write_state("swarm-branches", &state);
        Ok(entry)
    }

    pub fn list_branches() -> Vec<Value> {
        read_state("swarm-branches")["branches"].as_array().cloned().unwrap_or_default()
    }

    /// Merge a branch back into parent by overwriting the parent RVF store.
    pub fn merge_branch(name: &str, parent: &str) -> Result<Value, String> {
        let branch_dir = root().join(".claude-flow/branches");
        let safe_name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect(); let branch_path = branch_dir.join(format!("{safe_name}.rvf"));
        if !branch_path.exists() {
            return Err(format!("branch '{name}' not found"));
        }
        fs::copy(&branch_path, parent).map_err(|e| e.to_string())?;
        let entry = json!({"merged": name, "into": parent, "at": now_ms()});
        let mut state = read_state("swarm-branches");
        if let Some(arr) = state["branches"].as_array_mut() {
            arr.retain(|b| b["name"].as_str() != Some(name));
        }
        write_state("swarm-branches", &state);
        Ok(entry)
    }

    /// Query vectors in a branch (opens the branch RVF store for k-NN).
    pub fn query_branch(name: &str, query_vec: &[f32], limit: usize) -> Result<Value, String> {
        let branch_dir = root().join(".claude-flow/branches");
        let safe_name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect(); let branch_path = branch_dir.join(format!("{safe_name}.rvf"));
        if !branch_path.exists() {
            return Err(format!("branch '{name}' not found"));
        }
        let config = ruflo_storage::AgentDbFixtureConfig::new(384);
        let store = ruflo_storage::RvfPersistencePort::open_agentdb(&branch_path, config)
            .map_err(|e| e.to_string())?;
        let matches = store.search_agentdb(query_vec, limit).map_err(|e| e.to_string())?;
        let results: Vec<Value> = matches.iter().map(|m| {
            json!({"id": m.id, "distance": m.distance})
        }).collect();
        Ok(json!({"branch": name, "matches": results.len(), "results": results}))
    }
