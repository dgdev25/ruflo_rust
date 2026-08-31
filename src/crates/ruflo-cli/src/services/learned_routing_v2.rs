//! Auto-split from services.rs
use super::*;

    use super::*;
    use std::collections::HashMap;

    /// Rank agents for a task using discriminative scoring: within-agent
    /// success × cross-agent rarity (agents that uniquely succeed on a task
    /// keyword get boosted).
    pub fn rank_agents(task: &str, agents: &[String]) -> Vec<(String, f64)> {
        let state = read_state("learned-routing");
        let history = state["history"].as_array().cloned().unwrap_or_default();
        // Count per-agent success on tasks containing the same keywords.
        let task_lower = task.to_lowercase();
        let keywords: Vec<&str> = task_lower.split_whitespace().collect();
        let mut scores: HashMap<String, f64> = HashMap::new();
        let mut keyword_counts: HashMap<String, u32> = HashMap::new();
        for h in &history {
            let hist_task = h["task"].as_str().unwrap_or("").to_lowercase();
            let agent = h["agent"].as_str().unwrap_or("").to_string();
            let success = h["success"].as_bool().unwrap_or(false);
            // Check if any keyword overlaps.
            let overlap = keywords.iter().any(|k| hist_task.contains(k));
            if overlap {
                let score = if success { 1.0 } else { -0.5 };
                *scores.entry(agent.clone()).or_insert(0.0) += score;
                *keyword_counts.entry(agent).or_insert(0) += 1;
            }
        }
        // Rarity: agents that appear FEWER times for this keyword get a rarity boost.
        let total: u32 = keyword_counts.values().sum();
        let mut ranked: Vec<(String, f64)> = agents.iter().map(|a| {
            let base = *scores.get(a).unwrap_or(&0.0);
            let count = *keyword_counts.get(a).unwrap_or(&0);
            let rarity = if total > 0 && count > 0 {
                1.0 - (count as f64 / total as f64)
            } else { 0.5 }; // unknown agent → neutral rarity
            (a.clone(), base * (0.7 + 0.3 * rarity))
        }).collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }
