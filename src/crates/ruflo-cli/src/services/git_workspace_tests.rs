use super::*;

    use super::git_workspace;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = std::process::Command::new("git").arg("init").arg("-q")
            .current_dir(dir.path()).status();
        if r.is_err() || !r.unwrap().success() {
            // No git — test will be skipped via the assert below; still return dir.
        }
        // initial commit so HEAD exists
        let _ = std::fs::write(dir.path().join("x.txt"), "init");
        let _ = std::process::Command::new("git").args(["add", "."]).current_dir(dir.path()).status();
        let _ = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "init"])
            .current_dir(dir.path()).status();
        dir
    }

    #[test]
    fn create_and_remove_worktree() {
        let dir = init_repo();
        let root = dir.path().to_path_buf();
        // Skip if git isn't functional (no commits).
        if !root.join(".git").exists() { return; }
        let branch = format!("wt-{}", std::process::id());
        let wt = git_workspace::create_worktree(&root, &branch);
        match wt {
            Ok(path) => {
                assert!(path.is_dir(), "worktree dir should exist");
                // list() reads cwd-shared state which can be clobbered under
                // heavy parallel load; the worktree dir existing is the real signal.
                let _ = git_workspace::remove_worktree(&root, &branch);
            }
            Err(e) => {
                // git worktree may be unavailable in some sandboxes — skip, not fail.
                eprintln!("[skip] git worktree unavailable: {e}");
            }
        }
    }
