//! Auto-split from services.rs
use super::*;

    use super::*;

    /// Derive a cross-worktree job key: sha256(repoId, HEAD, workerType, configHash).
    /// This is stable across worktrees of the same repo — the #2661 fanout fix.
    /// Falls back to the caller-supplied job_id when git identity isn't resolvable.
    pub fn job_key(worker_type: &str, config_hash: &str) -> String {
        use sha2::{Digest, Sha256};
        let repo_id = git_repo_id().unwrap_or_else(|| "unknown".into());
        let head = git_head().unwrap_or_else(|| "unknown".into());
        let mut h = Sha256::new();
        h.update(repo_id.as_bytes());
        h.update(b"|");
        h.update(head.as_bytes());
        h.update(b"|");
        h.update(worker_type.as_bytes());
        h.update(b"|");
        h.update(config_hash.as_bytes());
        let digest = h.finalize();
        format!("job-{}", hex_encode(&digest))
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn git_repo_id() -> Option<String> {
        let out = std::process::Command::new("git")
            .args(["config", "--get", "remote.origin.url"])
            .output().ok()?;
        if !out.status.success() { return None; }
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if url.is_empty() { return None; }
        Some(url)
    }

    fn git_head() -> Option<String> {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output().ok()?;
        if !out.status.success() { return None; }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Check if a job is already in-flight using the cross-worktree job key.
    pub fn check_key(worker_type: &str, config_hash: &str) -> bool {
        let key = job_key(worker_type, config_hash);
        check(&key)
    }

    /// Mark a job as in-flight using the cross-worktree job key.
    pub fn mark_key(worker_type: &str, config_hash: &str) -> String {
        let key = job_key(worker_type, config_hash);
        mark(&key);
        key
    }

    pub fn check(job_id: &str) -> bool {
        let state = read_state("ai-job-dedup");
        state["jobs"][job_id].as_u64().is_some()
    }

    pub fn mark(job_id: &str) {
        let mut state = read_state("ai-job-dedup");
        if state["jobs"].is_null() {
            state["jobs"] = json!({});
        }
        state["jobs"][job_id] = json!(now_ms());
        write_state("ai-job-dedup", &state);
    }

    pub fn expire(max_age_ms: u64) {
        let now = now_ms();
        let mut state = read_state("ai-job-dedup");
        if let Some(jobs) = state["jobs"].as_object_mut() {
            jobs.retain(|_, v| {
                let at = v.as_u64().unwrap_or(0);
                now.saturating_sub(at) < max_age_ms
            });
        }
        write_state("ai-job-dedup", &state);
    }
