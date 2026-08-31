//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn propose(source: &str) -> Value {
        // Analyze the source (recent task outcomes) for improvement candidates.
        let decisions = std::fs::read_to_string(".claude-flow/router_decisions.jsonl").unwrap_or_default();
        let task_count = decisions.lines().count();
        let candidates = if task_count > 10 {
            vec![
                json!({"candidate": "route-optimization", "source": source, "rationale": "10+ decisions accumulated — retrain SONA"}),
                json!({"candidate": "budget-tuning", "source": source, "rationale": "adjust concurrent/hourly caps from observed spend"}),
            ]
        } else if task_count > 0 {
            vec![json!({"candidate": "baseline-collection", "source": source, "rationale": "collecting baseline data"})]
        } else {
            vec![]
        };
        let entry = json!({"proposals": candidates, "source": source, "decisionCount": task_count, "at": now_ms()});
        let mut state = read_state("flywheel-proposals");
        ensure_arr(&mut state, "proposals").push(entry.clone());
        write_state("flywheel-proposals", &state);
        entry
    }
