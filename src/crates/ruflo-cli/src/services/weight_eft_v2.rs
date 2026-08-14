//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn build_training_data(source: &str, quality: f64) -> Value {
        let entry = json!({
            "source": source, "quality": quality,
            "audited": quality > 0.5,
            "costEstimate": (1.0 - quality) * 0.5,
            "at": now_ms(),
        });
        let mut state = read_state("weight-eft");
        ensure_arr(&mut state, "trainingData").push(entry.clone());
        write_state("weight-eft", &state);
        entry
    }
    pub fn cost_pareto() -> Value {
        let data = read_state("weight-eft")["trainingData"].as_array().cloned().unwrap_or_default();
        let total_cost: f64 = data.iter()
            .filter_map(|d| d["costEstimate"].as_f64()).sum();
        let avg_quality = if data.is_empty() { 0.0 } else {
            data.iter().filter_map(|d| d["quality"].as_f64()).sum::<f64>() / data.len() as f64
        };
        json!({"samples": data.len(), "totalCost": total_cost, "avgQuality": avg_quality,
               "paretoOptimal": avg_quality / total_cost.max(0.001)})
    }
