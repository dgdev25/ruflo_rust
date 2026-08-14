//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn record(task: &str, agent: &str, success: bool) {
        let mut state = read_state("learned-routing");
        let key = format!("routes.{}", task.to_lowercase().chars().take(30).collect::<String>());
        let entry = json!({"agent": agent, "success": success, "at": now_ms()});
        state["routes"] = json!({}); let routes = state["routes"].as_object_mut().unwrap();
        let history = routes
            .get(&task.to_lowercase())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut hist = history;
        hist.push(entry);
        routes.insert(task.to_lowercase(), json!(hist));
        write_state("learned-routing", &state);
    }

    pub fn best_agent(task: &str) -> Option<String> {
        let state = read_state("learned-routing");
        let history = state["routes"][task.to_lowercase()].as_array()?;
        let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
        for h in history {
            let agent = h["agent"].as_str()?;
            let success = h["success"].as_bool().unwrap_or(false);
            let entry = counts.entry(agent.into()).or_insert((0, 0));
            entry.0 += 1;
            if success {
                entry.1 += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(_, (total, wins))| (*wins * 100) / (*total).max(1))
            .map(|(agent, _)| agent)
    }
