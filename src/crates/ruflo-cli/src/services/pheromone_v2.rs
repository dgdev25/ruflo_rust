//! Auto-split from services.rs
use super::*;

    use super::*;
    /// Check if an agent is eligible for dispatch (above the APSC threshold).
    /// Returns false if the agent's EMA fitness is below threshold → suspend.
    pub fn is_eligible(agent_id: &str) -> bool {
        let state = read_state("pheromone-adaptive");
        let threshold = state["threshold"].as_f64().unwrap_or(0.3);
        let fitness = state["agents"][agent_id]["emaFitness"].as_f64();
        match fitness {
            Some(f) => f >= threshold,
            None => true, // unknown agent → eligible (no history to penalize)
        }
    }
    /// Suspend an agent manually.
    pub fn suspend(agent_id: &str, reason: &str) -> Value {
        let mut state = read_state("pheromone-adaptive");
        if state["agents"][agent_id].is_null() {
            state["agents"][agent_id] = json!({});
        }
        state["agents"][agent_id]["suspended"] = json!(true);
        state["agents"][agent_id]["suspendReason"] = json!(reason);
        state["agents"][agent_id]["suspendedAt"] = json!(now_ms());
        write_state("pheromone-adaptive", &state);
        json!({"agent": agent_id, "suspended": true, "reason": reason})
    }
    /// Reactivate a suspended agent.
    pub fn reactivate(agent_id: &str) -> Value {
        let mut state = read_state("pheromone-adaptive");
        if !state["agents"][agent_id].is_null() {
            state["agents"][agent_id]["suspended"] = json!(false);
            state["agents"][agent_id]["reactivatedAt"] = json!(now_ms());
        }
        write_state("pheromone-adaptive", &state);
        json!({"agent": agent_id, "reactivated": true})
    }
