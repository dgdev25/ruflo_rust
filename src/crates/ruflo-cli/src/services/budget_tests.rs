use super::*;

    use super::global_budget;
    use std::sync::Mutex;
    static BUDGET_LOCK: Mutex<()> = Mutex::new(());

    fn fresh_state() {
        let _ = std::fs::remove_file(super::state_path("global-budget"));
    }

    #[test]
    fn record_books_cost() {
        let _g = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        fresh_state();
        let rec = global_budget::record("sonnet", 50000, true);
        assert!(rec["costUsd"].as_f64().unwrap_or(0.0) > 0.0, "record should book cost");
        assert!(rec["model"].as_str() == Some("sonnet"));
    }

    #[test]
    fn status_returns_object() {
        let _g = BUDGET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let st = global_budget::status();
        assert!(st.is_object());
        assert!(st["maxConcurrent"].is_u64() || st["maxConcurrent"].is_null());
    }
