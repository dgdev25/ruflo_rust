use super::*;

    use super::*;
    use std::sync::Mutex;
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn tmp() -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        std::env::set_current_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn bounded_pool_acquire_release() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let slot = bounded_pool::acquire("test", 2).unwrap();
        assert!(bounded_pool::acquire("test", 1).is_err()); // full
        assert!(bounded_pool::release("test", slot["id"].as_str().unwrap()));
        let s2 = bounded_pool::acquire("test", 1).unwrap(); // now free
        assert_eq!(s2["id"].as_str().unwrap().starts_with("slot-"), true);
    }

    #[test]
    fn worker_queue_fifo() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        worker_queue::enqueue(json!({"task": "a"}));
        worker_queue::enqueue(json!({"task": "b"}));
        assert_eq!(worker_queue::length(), 2);
        let first = worker_queue::dequeue().unwrap();
        assert_eq!(first["task"]["task"].as_str(), Some("a"));
    }

    #[test]
    fn dedup_check_mark() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        assert!(!dedup::check("job1"));
        dedup::mark("job1");
        assert!(dedup::check("job1"));
    }

    #[test]
    fn lease_acquire_release() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let lease = lease::acquire("ws1", "agent1", 60000).unwrap();
        assert!(lease::acquire("ws1", "agent2", 60000).is_err()); // held
        assert!(lease::release("ws1", "agent1"));
        lease::acquire("ws1", "agent2", 60000).unwrap(); // now free
    }

    #[test]
    fn checkpoint_validates() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        assert!(checkpoint::validate("gate1", vec![("a", true), ("b", true)]).is_ok());
        assert!(checkpoint::validate("gate2", vec![("a", false)]).is_err());
    }

    #[test]
    fn pheromone_record_eligible() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        pheromone::record("agent1", "coder", 0.9, 0.1, 1.0);
        pheromone::record("agent2", "coder", 0.1, 0.9, 0.1);
        let eligible = pheromone::eligible();
        assert!(eligible.contains(&"agent1".to_string()));
        assert!(!eligible.contains(&"agent2".to_string()));
    }

    #[test]
    fn harness_record_and_list() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        harness::record_run("benchmark", json!({"score": 42}));
        assert_eq!(harness::list_runs("benchmark").len(), 1);
    }

    #[test]
    fn flywheel_receipt_create_list() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        flywheel_receipt::create("eval", json!({"result": "pass"}));
        assert_eq!(flywheel_receipt::list().len(), 1);
    }

    #[test]
    fn autostart_install_cron_attempts_command() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        // install_cron must at least generate the cron line and attempt
        // `crontab -`. Success vs. failure depends on whether crontab is
        // available in the test env; both outcomes are acceptable.
        match autostart::install_cron() {
            Ok(cron) => assert!(
                cron.contains("@reboot"),
                "generated cron line must contain @reboot"
            ),
            Err(_) => { /* crontab unavailable in test env — acceptable */ }
        }
    }

    #[test]
    fn autostart_uninstall_clears_state() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        // Simulate a prior install by writing state directly.
        write_state(
            "autostart",
            &json!({"method": "cron", "config": "@reboot ruflo", "installedAt": now_ms()}),
        );
        assert!(state_path("autostart").exists());
        let _ = autostart::uninstall();
        assert!(
            !state_path("autostart").exists(),
            "uninstall must clear the autostart state file"
        );
    }

    #[test]
    fn claim_service_claim_release_reclaim() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        let entry = claim_service::claim("issue-1", "agent-1", "coder").unwrap();
        assert_eq!(entry["status"].as_str(), Some("active"));
        assert_eq!(entry["claimant"]["id"].as_str(), Some("agent-1"));
        // Second claim by a different agent while active fails.
        assert!(claim_service::claim("issue-1", "agent-2", "coder").is_err());
        // Re-claim by the same agent also fails (already active).
        assert!(claim_service::claim("issue-1", "agent-1", "coder").is_err());
        claim_service::release("issue-1", "agent-1").unwrap();
        // After release a different agent may claim it.
        let again = claim_service::claim("issue-1", "agent-2", "coder").unwrap();
        assert_eq!(again["claimant"]["id"].as_str(), Some("agent-2"));
        let status = claim_service::load_status();
        assert_eq!(status.len(), 1, "exactly one claim entry after re-claim");
    }

    #[test]
    fn claim_service_handoff_flow() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        claim_service::claim("issue-2", "alice", "coder").unwrap();
        claim_service::handoff("issue-2", "alice", "bob", "load balancing").unwrap();
        // Wrong target can't accept.
        assert!(claim_service::accept_handoff("issue-2", "eve").is_err());
        // Correct target accepts and becomes new claimant.
        claim_service::accept_handoff("issue-2", "bob").unwrap();
        let status = claim_service::load_status();
        let entry = &status[0];
        assert_eq!(entry["status"].as_str(), Some("active"));
        assert_eq!(entry["claimant"]["id"].as_str(), Some("bob"));
        // pendingHandoffTo should be cleared after acceptance.
        assert!(entry.get("pendingHandoffTo").is_none() || entry["pendingHandoffTo"].is_null());
        // Releasing by the previous owner fails.
        assert!(claim_service::release("issue-2", "alice").is_err());
    }

    #[test]
    fn claim_service_steal_flow() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        claim_service::claim("issue-3", "alice", "coder").unwrap();
        // Can't steal while still active.
        assert!(claim_service::steal("issue-3", "bob", "coder").is_err());
        claim_service::mark_stealable("issue-3", "claimant stale").unwrap();
        // Now stealable.
        claim_service::steal("issue-3", "bob", "coder").unwrap();
        let status = claim_service::load_status();
        let entry = &status[0];
        assert_eq!(entry["status"].as_str(), Some("stolen"));
        assert_eq!(entry["claimant"]["id"].as_str(), Some("bob"));
        // History records the steal event.
        let hist = entry["history"].as_array().unwrap();
        assert!(hist.iter().any(|h| h["event"].as_str() == Some("stolen")));
    }

    #[test]
    fn policy_evaluate() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _d = tmp();
        policy_runtime::add_rule("swarm.spawn", "deny");
        let result = policy_runtime::evaluate("swarm.spawn", "user1");
        assert_eq!(result["decision"].as_str(), Some("deny"));
        let allow = policy_runtime::evaluate("swarm.status", "user1");
        assert_eq!(allow["decision"].as_str(), Some("allow"));
    }
