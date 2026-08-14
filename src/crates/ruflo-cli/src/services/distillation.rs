//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn record_episode(source: &str, summary: &str, patterns: Vec<String>) -> Value {
        let mut state = read_state("memory-distillation");
        let episodes = ensure_arr(&mut state, "episodes");
        let episode = json!({
            "id": unique_id("ep"),
            "source": source,
            "summary": summary,
            "patterns": patterns,
            "createdAt": now_ms(),
        });
        episodes.push(episode.clone());
        write_state("memory-distillation", &state);
        episode
    }

    /// Run the full distillation pipeline: extract patterns from episodes,
    /// score them by frequency, build causal edges between co-occurring
    /// patterns, and consolidate into the distilled intelligence store.
    pub fn run_pipeline() -> Value {
        let mut state = read_state("memory-distillation");
        let episodes = state["episodes"].as_array().cloned().unwrap_or_default();
        if episodes.is_empty() {
            return json!({"status": "no_episodes", "distilled": 0});
        }

        // 1. EXTRACT: collect all patterns from episodes + their summaries.
        let mut pattern_counts: HashMap<String, u32> = HashMap::new();
        let mut pattern_sources: HashMap<String, Vec<String>> = HashMap::new();
        for ep in &episodes {
            let summary = ep["summary"].as_str().unwrap_or("");
            let source = ep["source"].as_str().unwrap_or("unknown").to_string();
            // Extract keywords from summary (word frequency).
            for word in summary.split_whitespace() {
                let word = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
                if word.len() < 3 { continue; }
                *pattern_counts.entry(word.clone()).or_insert(0) += 1;
                pattern_sources.entry(word).or_default().push(source.clone());
            }
            // Also use explicit patterns from the episode.
            if let Some(patterns) = ep["patterns"].as_array() {
                for p in patterns {
                    if let Some(s) = p.as_str() {
                        *pattern_counts.entry(s.to_string()).or_insert(0) += 2; // explicit patterns weight higher
                        pattern_sources.entry(s.to_string()).or_default().push(source.clone());
                    }
                }
            }
        }

        // 2. JUDGE: filter patterns by frequency threshold (≥2 occurrences).
        let threshold = 2u32;
        let significant: Vec<(String, u32)> = pattern_counts.iter()
            .filter(|(_, &count)| count >= threshold)
            .map(|(k, &v)| (k.clone(), v))
            .collect();

        // 3. DISTILL: build causal edges between co-occurring patterns.
        let mut edges = Vec::new();
        let sig_names: Vec<&str> = significant.iter().map(|(k, _)| k.as_str()).collect();
        for ep in &episodes {
            let summary = ep["summary"].as_str().unwrap_or("").to_lowercase();
            let present: Vec<&str> = sig_names.iter()
                .filter(|p| summary.contains(**p))
                .copied().collect();
            for i in 0..present.len() {
                for j in (i+1)..present.len() {
                    edges.push(json!({"from": present[i], "to": present[j], "weight": 1}));
                }
            }
        }

        // 4. CONSOLIDATE: write patterns + edges to the distilled store.
        let patterns_json: Vec<Value> = significant.iter()
            .map(|(name, count)| json!({
                "pattern": name,
                "frequency": count,
                "sources": pattern_sources.get(name).cloned().unwrap_or_default(),
            }))
            .collect();
        state["distilledPatterns"] = json!(patterns_json);
        state["causalEdges"] = json!(edges);
        state["lastDistilledAt"] = json!(now_ms());
        state["episodeCount"] = json!(episodes.len());
        write_state("memory-distillation", &state);

        json!({
            "status": "distilled",
            "episodes": episodes.len(),
            "patterns": patterns_json.len(),
            "edges": edges.len(),
            "backend": "native-keyword-frequency",
        })
    }

    pub fn patterns() -> Vec<Value> {
        read_state("memory-distillation")["distilledPatterns"]
            .as_array().cloned().unwrap_or_default()
    }

    pub fn causal_edges() -> Vec<Value> {
        read_state("memory-distillation")["causalEdges"]
            .as_array().cloned().unwrap_or_default()
    }

    pub fn list_episodes() -> Vec<Value> {
        read_state("memory-distillation")["episodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
