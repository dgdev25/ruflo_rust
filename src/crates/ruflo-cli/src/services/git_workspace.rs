//! Auto-split from services.rs
use super::*;

    use super::*;
    use std::process::Command;

    pub fn create_worktree(root: &Path, branch: &str) -> Result<PathBuf, String> {
        let wt_path = root.join(format!(".claude-flow/worktrees/{branch}"));
        let output = Command::new("git")
            .args(["worktree", "add", "-b", branch, wt_path.to_str().unwrap_or(""), "HEAD"])
            .current_dir(root)
            .output()
            .map_err(|e| format!("git worktree: {e}"))?;
        if !output.status.success() {
            // Worktree may already exist; check if path is valid.
            if !wt_path.is_dir() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
        }
        // Record in state.
        let mut state = read_state("git-worktrees");
        let entries = ensure_arr(&mut state, "worktrees");
        entries.push(json!({"branch": branch, "path": wt_path.display().to_string(), "createdAt": now_ms()}));
        write_state("git-worktrees", &state);
        Ok(wt_path)
    }

    pub fn remove_worktree(root: &Path, branch: &str) -> Result<(), String> {
        let wt_path = root.join(format!(".claude-flow/worktrees/{branch}"));
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", wt_path.to_str().unwrap_or("")])
            .current_dir(root)
            .output();
        // Also drop the branch (best-effort).
        let _ = Command::new("git")
            .args(["branch", "-D", branch])
            .current_dir(root)
            .output();
        let mut state = read_state("git-worktrees");
        if let Some(entries) = state["worktrees"].as_array_mut() {
            entries.retain(|w| w["branch"].as_str() != Some(branch));
        }
        write_state("git-worktrees", &state);
        // Release any lease held on this workspace (holder unknown here —
        // best-effort: try the recorded holder from state).
        let st = read_state("git-worktrees");
        let holder = st["worktrees"].as_array()
            .and_then(|arr| arr.iter().find(|w| w["branch"].as_str() == Some(branch)))
            .and_then(|w| w["holder"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let _ = crate::services::lease::release(&wt_path.display().to_string(), &holder);
        Ok(())
    }

    /// Acquire a worktree + a workspace lease atomically: each writing agent
    /// gets its own isolated git worktree owned by a time-limited lease.
    /// Returns (worktree_path, lease). The lease auto-releases on expiry; the
    /// worktree is removed explicitly via remove_worktree.
    pub fn acquire_with_lease(
        root: &Path,
        branch: &str,
        holder: &str,
        ttl_ms: u64,
    ) -> Result<(PathBuf, Value), String> {
        let wt = create_worktree(root, branch)?;
        // Record holder so remove_worktree can release the lease.
        let mut state = read_state("git-worktrees");
        if let Some(arr) = state["worktrees"].as_array_mut() {
            for w in arr.iter_mut() {
                if w["branch"].as_str() == Some(branch) {
                    w["holder"] = json!(holder);
                }
            }
        }
        write_state("git-worktrees", &state);
        let lease = crate::services::lease::acquire(&wt.display().to_string(), holder, ttl_ms)?;
        Ok((wt, lease))
    }

    pub fn list() -> Vec<Value> {
        read_state("git-worktrees")["worktrees"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
