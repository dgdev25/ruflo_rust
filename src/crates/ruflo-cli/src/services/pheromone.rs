//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn get_state() -> Value {
        read_state("pheromone-state")
    }

    pub fn record(agent_id: &str, role: &str, success: f64, latency_norm: f64, consensus: f64) {
        let mut state = read_state("pheromone-state");
        if state["version"].is_null() {
            state = json!({"version": "ruflo.apsc-state/v1", "threshold": 0.4, "agents": {}});
        }
        if !state["agents"].is_object() { state["agents"] = json!({}); }
        let agents = state["agents"].as_object_mut().expect("agents ensured as object");
        agents.insert(
            agent_id.into(),
            json!({
                "role": role,
                "emaSuccess": success,
                "emaLatency": latency_norm,
                "emaConsensus": consensus,
                "updatedAt": now_ms(),
            }),
        );
        write_state("pheromone-state", &state);
    }

    pub fn eligible() -> Vec<String> {
        let state = get_state();
        let threshold = state["threshold"].as_f64().unwrap_or(0.4);
        state["agents"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| {
                        let score = v["emaSuccess"].as_f64().unwrap_or(1.0)
                            * (1.0 - v["emaLatency"].as_f64().unwrap_or(0.0).abs())
                            * v["emaConsensus"].as_f64().unwrap_or(1.0);
                        score >= threshold
                    })
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
