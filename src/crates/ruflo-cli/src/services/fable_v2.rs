//! Auto-split from services.rs
use super::*;

    use super::*;
    /// Judge a trajectory by spawning claude/codex to evaluate it.
    pub fn judge(trajectory: &str, criteria: &str) -> Value {
        let prompt = format!("Judge this trajectory against: {criteria}\n\nTrajectory:\n{trajectory}\n\nRespond PASS or FAIL with one sentence reason.");
        let result = crate::services::headless::execute("fable-judge", "claude", &prompt, 60_000, &[]);
        let verdict = if result["status"].as_str() == Some("completed") {
            let stdout = result["stdout"].as_str().unwrap_or("");
            if stdout.to_lowercase().contains("pass") { "pass" }
            else if stdout.to_lowercase().contains("fail") { "fail" }
            else { "inconclusive" }
        } else { "error" };
        let entry = json!({"verdict": verdict, "criteria": criteria, "at": now_ms()});
        let mut state = read_state("fable-harness");
        ensure_arr(&mut state, "judgments").push(entry.clone());
        write_state("fable-harness", &state);
        entry
    }
