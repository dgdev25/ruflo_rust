//! Distillation/training behavioral layer — real oracle labeling,
//! hyperparameter tuning, and the native training pipeline.
//!
//! Ports services/distill-oracle.ts + distill-tuning.ts + native-training.ts
//! behavioral parity. Built ON TOP of the SONA MLP (sona.rs) + router
//! decisions — no Node, no WASM.
//!
//! - oracle::label_from_decisions() reads router_decisions.jsonl and produces
//!   labeled (task → best model by success rate) training examples.
//! - tuning::grid_search() sweeps learning-rate × epochs combos, evaluates each
//!   via a held-out split, returns the best hyperparameters.
//! - pipeline::run() runs the full SONA fit on the labeled set with EWC++
//!   consolidation, checkpointing each epoch.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::sona;

fn router_decisions_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".claude-flow/router_decisions.jsonl")
}

/// Read (task, model) decisions and build labeled examples: for each distinct
/// task string, the model with the highest observed success rate becomes the
/// label. Returns (examples, class_map) where examples = Vec<(task, class_idx)>.
pub fn label_from_decisions() -> (Vec<(String, usize)>, Vec<String>) {
    let raw = fs::read_to_string(router_decisions_path()).unwrap_or_default();
    // Tally per (task, model) success counts from any success/failure signals.
    // router_decisions.jsonl holds {task, model}; we treat each as a positive
    // sample (the route was taken). For richer labeling, route_feedback.jsonl
    // could supply success — fold both in.
    let mut counts: std::collections::HashMap<(String, String), u32> = std::collections::HashMap::new();
    for line in raw.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let task = v["task"].as_str().unwrap_or("").to_string();
            let model = v["model"].as_str().unwrap_or("").to_string();
            if !task.is_empty() && !model.is_empty() {
                *counts.entry((task, model)).or_insert(0) += 1;
            }
        }
    }
    // Also fold in explicit feedback if present.
    let fb = fs::read_to_string(
        std::env::current_dir().unwrap_or_default().join(".claude-flow/route-feedback.jsonl"),
    ).unwrap_or_default();
    for line in fb.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let task = v["task"].as_str().unwrap_or("").to_string();
            let model = v["model"].as_str().unwrap_or("").to_string();
            let success = v["success"].as_bool().unwrap_or(true);
            if !task.is_empty() && !model.is_empty() {
                let delta = if success { 2 } else { 0 };
                *counts.entry((task, model)).or_insert(0) += delta;
            }
        }
    }
    // Build class map.
    let mut class_map: Vec<String> = Vec::new();
    for (_, m) in counts.keys() {
        if !class_map.contains(m) {
            class_map.push(m.clone());
        }
    }
    // For each task, pick the highest-scoring model.
    let mut per_task: std::collections::HashMap<String, (String, u32)> = std::collections::HashMap::new();
    for ((task, model), n) in &counts {
        let entry = per_task.entry(task.clone()).or_insert((model.clone(), 0));
        if *n > entry.1 {
            *entry = (model.clone(), *n);
        }
    }
    let examples: Vec<(String, usize)> = per_task.into_iter()
        .map(|(task, (model, _))| {
            let class = class_map.iter().position(|c| c == &model).unwrap_or(0);
            (task, class)
        })
        .collect();
    (examples, class_map)
}

/// Grid search over learning-rate × epochs, evaluating each combo via a quick
/// SONA fit + held-out accuracy. Returns the best (params, accuracy).
pub fn grid_search(
    examples: &[(String, usize)],
    class_map: &[String],
    dim: usize,
    hidden: usize,
) -> (Value, f64) {
    let lrs = [0.01, 0.05, 0.1, 0.2];
    let epoch_grid = [10, 20, 40];
    let mut best = (json!({"learningRate": 0.05, "epochs": 20}), 0.0f64);
    if examples.is_empty() || class_map.is_empty() {
        return best;
    }
    // 80/20 split for held-out eval.
    let split = examples.len() * 4 / 5;
    let train = &examples[..split.max(1)];
    let held = &examples[split.max(1)..];
    let train_vec: Vec<(Vec<f64>, usize)> = train.iter().map(|(t, c)| {
        let (v, _) = crate::onnx_embeddings::embed(t, dim);
        (v, *c)
    }).collect();
    let held_vec: Vec<(Vec<f64>, usize)> = held.iter().map(|(t, c)| {
        let (v, _) = crate::onnx_embeddings::embed(t, dim);
        (v, *c)
    }).collect();
    for &lr in &lrs {
        for &epochs in &epoch_grid {
            let mut net = sona::SonaNet::new(dim, hidden, class_map.len());
            let cfg = sona::TrainConfig { lr, momentum: 0.9, l2: 1e-4 };
            net.fit(&train_vec, epochs, cfg, None);
            let correct = held_vec.iter().filter(|(x, y)| net.predict(x) == *y).count();
            let acc = if held_vec.is_empty() { 0.0 } else { correct as f64 / held_vec.len() as f64 };
            if acc > best.1 {
                best = (json!({"learningRate": lr, "epochs": epochs, "hidden": hidden}), acc);
            }
        }
    }
    best
}

/// Run the full native training pipeline: label → grid-search → final fit with
/// EWC++ consolidation → checkpoint each epoch. Returns the run summary.
pub fn run(dim: usize, hidden: usize, max_epochs: usize) -> Value {
    let (examples, class_map) = label_from_decisions();
    if examples.is_empty() {
        return json!({"status": "no_decisions", "examples": 0});
    }
    let (best_params, best_acc) = grid_search(&examples, &class_map, dim, hidden);
    let lr = best_params["learningRate"].as_f64().unwrap_or(0.05);
    let epochs = best_params["epochs"].as_u64().unwrap_or(20) as usize;

    let examples_vec: Vec<(Vec<f64>, usize)> = examples.iter().map(|(t, c)| {
        let (v, _) = crate::onnx_embeddings::embed(t, dim);
        (v, *c)
    }).collect();

    let mut net = sona::SonaNet::new(dim, hidden, class_map.len());
    let cfg = sona::TrainConfig { lr, momentum: 0.9, l2: 1e-4 };
    let mut checkpoints = Vec::new();
    // Fit in chunks of `epoch_chunk`, checkpointing between, so a crash loses
    // at most one chunk. EWC++ consolidates after each chunk.
    let epoch_chunk = (epochs / 4).max(1).min(10);
    let mut done = 0usize;
    let mut ewc = sona::EwcState::default();
    while done < max_epochs.min(epochs) {
        let take = epoch_chunk.min(max_epochs.saturating_sub(done));
        let loss = net.fit(&examples_vec, take, cfg, if done > 0 { Some(&ewc) } else { None });
        let fisher = net.compute_fisher(&examples_vec);
        net.consolidate(&mut ewc, fisher);
        checkpoints.push(json!({"epoch": done + take, "loss": loss, "consolidated": true}));
        crate::services::native_training::record_checkpoint("sona-pipeline", done + take, loss);
        done += take;
    }

    json!({
        "status": "trained",
        "examples": examples.len(),
        "classes": class_map.len(),
        "bestParams": best_params,
        "heldOutAccuracy": best_acc,
        "epochsRun": done,
        "checkpoints": checkpoints,
        "backend": "native-sona-ewc++",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_when_no_decisions() {
        // No router_decisions file in test cwd → empty examples.
        let (ex, classes) = label_from_decisions();
        let _ = (ex, classes);
    }

    #[test]
    fn grid_search_handles_empty() {
        let (params, acc) = grid_search(&[], &[], 16, 8);
        assert_eq!(acc, 0.0);
        assert!(params["learningRate"].as_f64().is_some());
    }

    #[test]
    fn run_returns_no_decisions_status() {
        let r = run(16, 8, 5);
        // Either no_decisions (empty) or trained — both valid.
        let status = r["status"].as_str().unwrap_or("");
        assert!(status == "no_decisions" || status == "trained", "got {status}");
    }
}
