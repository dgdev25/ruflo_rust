//! Auto-split from services.rs
use super::*;

    use super::*;
    /// Compute a stable repo identity from git config (shared across worktrees).
    /// This is the #2661 fanout fix: all worktrees of the same repo get the same identity.
    pub fn repo_identity() -> String {
        // Try git common-dir (shared across worktrees).
        let common_dir = std::process::Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        let remote = std::process::Command::new("git")
            .args(["config", "--get", "remote.origin.url"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        // Identity = SHA-256 of (remote URL or common-dir).
        use sha2::{Digest, Sha256};
        let id_source = remote.or(common_dir).unwrap_or_else(|| "unknown".into());
        let hash = Sha256::digest(id_source.as_bytes());
        hash.iter().map(|b| format!("{b:02x}")).collect::<String>()[..16].to_string()
    }

    /// Check if two paths belong to the same repo (shared identity).
    pub fn same_repo(path_a: &str, path_b: &str) -> bool {
        let id_a = std::process::Command::new("git")
            .args(["-C", path_a, "config", "--get", "remote.origin.url"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let id_b = std::process::Command::new("git")
            .args(["-C", path_b, "config", "--get", "remote.origin.url"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        !id_a.is_empty() && id_a == id_b
    }
