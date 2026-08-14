//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn record_run(harness_type: &str, data: Value) -> Value {
        let file = format!("harness-{harness_type}");
        let mut state = read_state(&file);
        let run = json!({
            "id": unique_id("run"),
            "data": data,
            "recordedAt": now_ms(),
        });
        ensure_arr(&mut state, "runs").push(run.clone());
        write_state(&file, &state);
        run
    }

    pub fn list_runs(harness_type: &str) -> Vec<Value> {
        let file = format!("harness-{harness_type}");
        read_state(&file)["runs"].as_array().cloned().unwrap_or_default()
    }

    pub fn get_state(harness_type: &str) -> Value {
        read_state(&format!("harness-{harness_type}"))
    }

    /// All harness service types (15 from TS).
    pub const HARNESS_TYPES: &[&str] = &[
        "loop", "benchmark", "canary", "replay", "verify", "worker",
        "hosts", "corpus-harvester", "frozen-eval", "improvement-ledger",
        "project-anchor", "qualification", "flywheel", "flywheel-runtime",
        "flywheel-generations",
    ];
