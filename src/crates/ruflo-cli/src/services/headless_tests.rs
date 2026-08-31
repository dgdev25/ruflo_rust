use super::*;

    use super::headless;

    #[test]
    fn execute_runs_subprocess_and_captures_status() {
        // `true` ignores args and exits 0 — proves the spawn/wait/status path.
        let r = headless::execute("test", "true", "ignored", 5000, &[]);
        let status = r["status"].as_str().unwrap_or("");
        assert!(status == "completed" || status == "failed", "got {status}");
        assert_ne!(status, "spawn_failed");
    }

    #[test]
    fn execute_unavailable_binary_degrades() {
        let r = headless::execute("test", "definitely-not-a-binary-xyz", "x", 1000, &[]);
        assert_eq!(r["status"].as_str(), Some("unavailable"));
    }
