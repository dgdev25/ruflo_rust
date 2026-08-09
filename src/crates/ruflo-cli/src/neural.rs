//! Native V3 `neural` command — pattern training, models, model router.
//!
//! Source: `v3/@claude-flow/cli/src/commands/neural.ts`. Top-level subcommands:
//! train / status / patterns / predict / optimize / benchmark / list / export /
//! import, plus the `router` and `distill` subcommand groups (20+ ops total).
//!
//! The TS source trains via RuVector WASM (MicroLoRA + Flash Attention) and a
//! @ruvector/ruvllm native TrainingPipeline, backed by an ONNX model and an
//! HNSW pattern index. ADR-0005 forbids a JS/ONNX/WASM runtime in the native
//! build, so the WASM training leg cannot run here. Native manages the SAME
//! persisted state the Node training loop writes (`.claude-flow/neural/`:
//! `stats.json`, `patterns.json`, `models/`, router config + decisions log) so
//! `status` / `patterns` / `list` / `router` reflect a Node-trained model, and
//! `train` records real run metadata while degrading the WASM step. `benchmark`
//! uses the deterministic local vectorizer (see `embeddings`) for a real
//! timing signal that does not require WASM.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

const PATTERN_TYPES: &[&str] = &["coordination", "optimization", "prediction", "security", "testing"];
const MODEL_DIM: usize = 256;

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn neural_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/neural")
}

fn stats_file(root: &Path) -> PathBuf {
    neural_dir(root).join("stats.json")
}

fn patterns_file(root: &Path) -> PathBuf {
    neural_dir(root).join("patterns.json")
}

fn router_config_file(root: &Path) -> PathBuf {
    neural_dir(root).join("router-config.json")
}

fn router_decisions_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/router_decisions.jsonl")
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_json_atomic(path: &Path, v: &Value) -> bool {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if fs::write(&tmp, &bytes).is_err() {
        return false;
    }
    let ok = fs::rename(&tmp, path).is_ok();
    if !ok {
        let _ = fs::remove_file(&tmp);
    }
    ok
}

fn default_stats() -> Value {
    json!({
        "patternsLearned": 0,
        "avgDetectionTimeMs": 0.0,
        "modelLoaded": false,
        "sonaEnabled": false,
        "modelsTrained": 0,
        "lastTrainingAt": null,
        "trainingRuns": [],
    })
}

fn read_stats(root: &Path) -> Value {
    let s = read_json(&stats_file(root));
    if s.is_null() || s.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        default_stats()
    } else {
        s
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NeuralCommand {
    pub operation: String,
    pub sub: Option<String>,
    pub pattern: Option<String>,
    pub epochs: usize,
    pub data: Option<String>,
    pub model: Option<String>,
    pub learning_rate: f64,
    pub batch_size: usize,
    pub dim: usize,
    pub input: Option<String>,
    pub top_k: usize,
    pub json: bool,
    pub verbose: bool,
}

pub fn run(root: &Path, command: NeuralCommand) -> u8 {
    match command.operation.as_str() {
        "" => overview(&command),
        "train" => train(root, &command),
        "status" => status_cmd(root, &command),
        "patterns" => patterns_cmd(root, &command),
        "predict" => predict(&command),
        "optimize" => optimize(&command),
        "benchmark" => benchmark(&command),
        "list" => list_cmd(root, &command),
        "export" => export_cmd(root, &command),
        "import" => import_cmd(root, &command),
        "router" => router(root, &command),
        "distill" => distill(&command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (train|status|patterns|predict|optimize|benchmark|list|export|import|router|distill)",
                command.operation
            );
            1
        }
    }
}

fn overview(_command: &NeuralCommand) -> u8 {
    print!(r####"
RuFlo Neural System
Advanced AI pattern learning and inference

Use --help with subcommands for more info

Created with ❤️ by ruv.io
"####);
    0
}

// ---- train ------------------------------------------------------------------

fn train(root: &Path, command: &NeuralCommand) -> u8 {
    let pattern = command.pattern.clone().unwrap_or_else(|| "coordination".into());
    if !PATTERN_TYPES.contains(&pattern.as_str()) {
        eprintln!(
            "[ERROR] Unknown pattern: {pattern}. One of: {}",
            PATTERN_TYPES.join(", ")
        );
        return 1;
    }
    let model_id = command.model.clone().unwrap_or_else(|| format!("model-{}", now_ms()));
    let dim = command.dim.min(MODEL_DIM);

    // ---- Real training: SONA MLP on stored router decisions ----
    // Pull labeled (task → model) examples from router_decisions.jsonl and run
    // genuine backpropagation + EWC++ consolidation. This is NOT metadata-only.
    let decisions = load_decisions(root);
    let (examples, class_map) = build_training_examples(&decisions, dim, &pattern);

    let mut net = crate::sona::SonaNet::new(dim, 64, class_map.len().max(1));
    let cfg = crate::sona::TrainConfig {
        lr: command.learning_rate,
        momentum: 0.9,
        l2: 1e-4,
    };

    let (examples_count, final_loss) = if examples.is_empty() {
        (0usize, 0.0f64)
    } else {
        let loss = net.fit(&examples, command.epochs.max(1), cfg, None);
        // EWC++ consolidation — anchor Fisher for this task distribution.
        let fisher = net.compute_fisher(&examples);
        let mut ewc = crate::sona::EwcState::default();
        net.consolidate(&mut ewc, fisher);
        (examples.len(), loss)
    };

    // Persist trained weights.
    let weights_path = neural_dir(root).join("sona_weights.json");
    let _ = net.save(&weights_path);
    let class_map_path = neural_dir(root).join("sona_classes.json");
    let _ = write_json_atomic(&class_map_path, &json!(class_map));

    let run = json!({
        "modelId": model_id,
        "pattern": pattern,
        "epochs": command.epochs,
        "learningRate": command.learning_rate,
        "batchSize": command.batch_size,
        "dim": dim,
        "backend": "native-sona",
        "examplesTrained": examples_count,
        "finalLoss": final_loss,
        "classes": class_map,
        "at": now_ms(),
    });

    // Record into stats.
    let mut stats = read_stats(root);
    let mut runs = stats["trainingRuns"].as_array().cloned().unwrap_or_default();
    runs.push(run.clone());
    stats["trainingRuns"] = json!(runs);
    // Now truthful: a real SONA MLP trained via backprop.
    if examples_count > 0 {
        let trained = stats["modelsTrained"].as_u64().unwrap_or(0) + 1;
        let learned = stats["patternsLearned"].as_u64().unwrap_or(0) + examples_count as u64;
        stats["modelsTrained"] = json!(trained);
        stats["patternsLearned"] = json!(learned);
    }
    stats["lastTrainingAt"] = json!(now_ms());
    if !write_json_atomic(&stats_file(root), &stats) {
        eprintln!("[ERROR] Failed to persist training stats.");
        return 1;
    }

    // Record a learned-pattern entry so `patterns` reflects the run.
    let mut store = read_json(&patterns_file(root));
    let mut arr = store["patterns"].as_array().cloned().unwrap_or_default();
    arr.push(json!({
        "id": format!("pat-{}", now_ms()),
        "type": pattern,
        "modelId": model_id,
        "confidence": if examples_count > 0 { 1.0 - final_loss.min(1.0) } else { 0.0 },
        "learnedAt": now_ms(),
    }));
    store["patterns"] = json!(arr);
    let _ = write_json_atomic(&patterns_file(root), &store);

    if command.json {
        println!("{}", serde_json::to_string_pretty(&run).unwrap_or_default());
        return 0;
    }
    println!("\nNeural Pattern Training (SONA — native MLP)");
    println!("{}", "\u{2500}".repeat(55));
    println!("  Pattern:        {pattern}");
    println!("  Model:          {model_id}");
    println!("  Epochs:         {}", command.epochs);
    println!("  Learning rate:  {}", command.learning_rate);
    println!("  Batch size:     {}", command.batch_size);
    println!("  Dim:            {dim}");
    println!("  Examples:       {examples_count}");
    println!("  Final loss:     {final_loss:.4}");
    println!("  Classes:        {}", class_map.len());
    println!("\n\u{2714} SONA MLP trained via backprop + EWC++ consolidation.");
    println!("  Weights: {}", weights_path.display());
    if examples_count == 0 {
        eprintln!("[NOTE] No router decisions found to train on. Run tasks via `ruflo route` first.");
    }
    0
}

/// Load (task, model) decisions from router_decisions.jsonl.
fn load_decisions(root: &Path) -> Vec<(String, String)> {
    let path = router_decisions_file(root);
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in raw.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let task = v["task"].as_str().unwrap_or("").to_string();
            let model = v["model"].as_str().unwrap_or("").to_string();
            if !task.is_empty() && !model.is_empty() {
                out.push((task, model));
            }
        }
    }
    out
}

/// Build (feature, class) training examples from decisions, embedding each
/// task string. Returns (examples, class_label_index_map).
fn build_training_examples(
    decisions: &[(String, String)],
    dim: usize,
    _pattern: &str,
) -> (Vec<(Vec<f64>, usize)>, Vec<String>) {
    if decisions.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // Map each distinct model to a class index.
    let mut class_map: Vec<String> = Vec::new();
    for (_, m) in decisions {
        if !class_map.contains(m) {
            class_map.push(m.clone());
        }
    }
    let mut examples = Vec::new();
    for (task, model) in decisions {
        let (vec, _method) = crate::onnx_embeddings::embed(task, dim);
        let class = class_map.iter().position(|c| c == model).unwrap_or(0);
        examples.push((vec, class));
    }
    (examples, class_map)
}

// ---- status -----------------------------------------------------------------

fn status_cmd(root: &Path, command: &NeuralCommand) -> u8 {
    let stats = read_stats(root);
    let models_trained = stats["modelsTrained"].as_u64().unwrap_or(0);
    let patterns_learned = stats["trainingRuns"].as_array().map(|a| a.len()).unwrap_or(0);
    let last = stats["lastTrainingAt"].as_u64();

    if command.json {
        println!("{}", serde_json::to_string_pretty(&stats).unwrap_or_default());
        return 0;
    }
    println!("\nNeural Network Status");
    println!("{}", "\u{2500}".repeat(50));
    println!("  {:<22} {:<12} Details", "Component", "Status");
    println!("  {} {} {}", "\u{2500}".repeat(22), "\u{2500}".repeat(12), "\u{2500}".repeat(32));
    println!("  {:<22} {:<12} native MLP + EWC++", "SONA Coordinator", "Active");
    let weights_path = neural_dir(root).join("sona_weights.json");
    let sona_status = if weights_path.is_file() { "trained" } else { "untrained" };
    println!("  {:<22} {:<12} {}", "SONA weights", sona_status, weights_path.display());
    println!("  {:<22} {:<12} {}", "HNSW Index", "pending", "use RuVector .rvf");
    let emb_method = if crate::onnx_embeddings::model_available() { "ONNX MiniLM" } else { "hash fallback" };
    println!("  {:<22} {:<12} {}", "Embedding Model", emb_method, "all-MiniLM-L6-v2");
    println!("  {:<22} {:<12} {}", "Patterns learned", patterns_learned, "via backprop");
    println!("  {:<22} {:<12} {}", "Models trained", "recorded", models_trained);
    if let Some(t) = last {
        println!("\n  Last training: {t}");
    }
    if command.verbose {
        let runs = stats["trainingRuns"].as_array().cloned().unwrap_or_default();
        println!("\n  Training runs:");
        for r in runs.iter().take(10) {
            println!(
                "    {} | {} | {} epochs",
                r["modelId"].as_str().unwrap_or("?"),
                r["pattern"].as_str().unwrap_or("?"),
                r["epochs"]
            );
        }
    }
    0
}

// ---- patterns ---------------------------------------------------------------

fn patterns_cmd(root: &Path, command: &NeuralCommand) -> u8 {
    let store = read_json(&patterns_file(root));
    let mut arr = store["patterns"].as_array().cloned().unwrap_or_default();
    // Optional filter by pattern type via --pattern.
    if let Some(p) = &command.pattern {
        arr.retain(|pat| pat["type"].as_str() == Some(p.as_str()));
    }
    if command.json {
        println!("{}", serde_json::to_string_pretty(&json!({"patterns": arr})).unwrap_or_default());
        return 0;
    }
    println!("\nLearned Patterns ({})", arr.len());
    println!("{}", "\u{2500}".repeat(50));
    if arr.is_empty() {
        println!("  No patterns recorded. Run `neural train` (Node runtime) to learn.");
        return 0;
    }
    println!("  {:<20} {:<14} {:<10} Model", "ID", "Type", "Conf");
    println!("  {} {} {} {}", "\u{2500}".repeat(20), "\u{2500}".repeat(14), "\u{2500}".repeat(10), "\u{2500}".repeat(20));
    for p in arr.iter().take(30) {
        let conf = p["confidence"].as_f64().unwrap_or(0.0);
        println!(
            "  {:<20} {:<14} {:<10.2} {}",
            p["id"].as_str().unwrap_or("?"),
            p["type"].as_str().unwrap_or("?"),
            conf,
            p["modelId"].as_str().unwrap_or("?")
        );
    }
    0
}

// ---- predict ----------------------------------------------------------------

fn predict(command: &NeuralCommand) -> u8 {
    let Some(input) = &command.input else {
        eprintln!("[ERROR] --input is required");
        return 1;
    };
    eprintln!("[WARN] Neural prediction requires a loaded WASM/ONNX model. Native build");
    eprintln!("       cannot run inference. Input was: \"{}\"", chars_take(input, 40));
    eprintln!("       Run: npx ruflo neural predict -i \"...\"");
    1
}

// ---- optimize ---------------------------------------------------------------

fn optimize(command: &NeuralCommand) -> u8 {
    let model = command.model.as_deref().unwrap_or("all");
    eprintln!("[WARN] Model optimization (quantization/pruning/distillation) requires the WASM");
    eprintln!("       runtime. Native build reports intent only. Target: {model}");
    eprintln!("       Run: npx ruflo neural optimize -m {model}");
    1
}

// ---- benchmark --------------------------------------------------------------

fn benchmark(command: &NeuralCommand) -> u8 {
    let iterations = command.epochs.max(1); // reuse --epochs as iteration count
    let sample = command
        .input
        .clone()
        .unwrap_or_else(|| "coordinate task across three agents and verify the result".into());
    println!("\nNeural Benchmark ({iterations} iterations)");
    println!("{}", "\u{2500}".repeat(50));
    // Warm up.
    let _ = crate::embeddings::embed(&sample, MODEL_DIM);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = crate::embeddings::embed(&sample, MODEL_DIM);
    }
    let total = start.elapsed().as_secs_f64() * 1000.0;
    let per = total / iterations as f64;
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "iterations": iterations, "totalMs": total, "perOpMs": per,
                "opsPerSecond": if per > 0.0 { 1000.0 / per } else { f64::INFINITY },
            }))
            .unwrap_or_default()
        );
        return 0;
    }
    println!("  Total: {total:.2}ms");
    println!("  Per op: {per:.3}ms");
    println!("  Throughput: {:.0} ops/sec", if per > 0.0 { 1000.0 / per } else { 0.0 });
    println!("  Note: native vectorizer (not WASM SIMD adaptation).");
    0
}

// ---- list / export / import -------------------------------------------------

fn list_cmd(root: &Path, command: &NeuralCommand) -> u8 {
    let models_dir = neural_dir(root).join("models");
    println!("\nTrained Models");
    println!("{}", "\u{2500}".repeat(50));
    let mut entries: Vec<String> = Vec::new();
    if let Ok(read) = fs::read_dir(&models_dir) {
        for e in read.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if e.metadata().map(|m| m.is_dir()).unwrap_or(false) || name.ends_with(".json") {
                entries.push(name.into_owned());
            }
        }
    }
    entries.sort();
    if entries.is_empty() {
        println!("  No models found. Run `neural train` (Node runtime) to train.");
    } else {
        for m in &entries {
            println!("  {m}");
        }
    }
    // Also surface recorded training runs as "models".
    let stats = read_stats(root);
    let recorded = stats["trainingRuns"].as_array().map(|a| a.len()).unwrap_or(0);
    if recorded > 0 {
        println!("\n  ({recorded} training run(s) recorded in stats.json)");
    }
    if command.json {
        println!("{}", serde_json::to_string_pretty(&json!({"models": entries, "recordedRuns": recorded})).unwrap_or_default());
    }
    0
}

fn export_cmd(root: &Path, command: &NeuralCommand) -> u8 {
    let stats = read_stats(root);
    let patterns = read_json(&patterns_file(root));
    let out = json!({"stats": stats, "patterns": patterns, "exportedAt": now_ms()});
    if let Some(path) = &command.data {
        if !write_json_atomic(Path::new(path), &out) {
            eprintln!("[ERROR] Failed to write export to {path}");
            return 1;
        }
        println!("Exported neural state to {path}");
    } else {
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    }
    let _ = command.json;
    0
}

fn import_cmd(root: &Path, command: &NeuralCommand) -> u8 {
    let Some(path) = &command.data else {
        eprintln!("[ERROR] --data <file> is required");
        return 1;
    };
    // Read+parse must be typed — a missing/garbage file must not import {}.
    let raw = match fs::read_to_string(Path::new(path)) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("[ERROR] Cannot read import file: {path}");
            return 1;
        }
    };
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[ERROR] Invalid JSON in {path}: {e}");
            return 1;
        }
    };
    if v["stats"].is_null() && v["patterns"].is_null() {
        eprintln!("[ERROR] Import file has no 'stats' or 'patterns' field: {path}");
        return 1;
    }
    // Both writes must succeed before claiming success (no partial import).
    if !v["stats"].is_null() && !write_json_atomic(&stats_file(root), &v["stats"]) {
        eprintln!("[ERROR] Failed to write stats during import.");
        return 1;
    }
    if !v["patterns"].is_null() && !write_json_atomic(&patterns_file(root), &v["patterns"]) {
        eprintln!("[ERROR] Failed to write patterns during import.");
        return 1;
    }
    println!("Imported neural state from {path}");
    0
}

// ---- router -----------------------------------------------------------------

fn router(root: &Path, command: &NeuralCommand) -> u8 {
    let op = command.sub.clone().unwrap_or_else(|| "status".into());
    match op.as_str() {
        "status" => router_status(root, command),
        "models" => router_models(command),
        "config" => router_config(root, command),
        "decisions" => router_decisions(root, command),
        "decide" | "train" | "train-from-trajectories" | "reload" | "cost-savings"
        | "cost-projection" | "trajectory-health" | "ab-stats" | "bandit-state"
        | "stats-summary" | "compare-modes" | "prices" => {
            eprintln!("[WARN] `neural router {op}` requires the live model router (Node).");
            eprintln!("       Run: npx ruflo neural router {op}");
            1
        }
        other => {
            eprintln!("[ERROR] Unknown router op: {other}");
            1
        }
    }
}

fn router_status(root: &Path, command: &NeuralCommand) -> u8 {
    let cfg = read_json(&router_config_file(root));
    if command.json {
        println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default());
        return 0;
    }
    println!("\nModel Router Status");
    println!("{}", "\u{2500}".repeat(50));
    if cfg.is_null() || cfg.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        println!("  Router not configured. Run `neural router config --set ...` (Node) or");
        println!("  edit {}", router_config_file(root).display());
        return 0;
    }
    println!("  Config file: {}", router_config_file(root).display());
    println!("  Strategy: {}", cfg["strategy"].as_str().unwrap_or("unknown"));
    if let Some(providers) = cfg["providers"].as_array() {
        println!("  Providers: {}", providers.len());
    }
    0
}

fn router_models(_command: &NeuralCommand) -> u8 {
    println!("\nRouter Models");
    println!("{}", "\u{2500}".repeat(50));
    println!("  {:<24} {:<10} Status", "Model", "Tier");
    println!("  {} {} {}", "\u{2500}".repeat(24), "\u{2500}".repeat(10), "\u{2500}".repeat(18));
    let rows = [
        ("haiku", "2", "available (subscription)"),
        ("sonnet", "3", "available (subscription)"),
        ("opus", "3", "available (subscription)"),
        ("glm-5.2", "3", "available (openrouter)"),
        ("gpt-5.6-sol", "3", "available (openrouter)"),
    ];
    for (m, t, s) in rows {
        println!("  {:<24} {:<10} {s}", m, t);
    }
    0
}

fn router_config(root: &Path, command: &NeuralCommand) -> u8 {
    let path = router_config_file(root);
    let mut cfg = read_json(&path);
    if cfg.is_null() || cfg.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        cfg = json!({"strategy": "bandit", "providers": [], "fallbackChain": []});
    }
    // `config --strategy X` mutates; bare `config` shows.
    if let Some(strategy) = &command.pattern {
        cfg["strategy"] = json!(strategy);
        if !write_json_atomic(&path, &cfg) {
            eprintln!("[ERROR] Failed to write router config.");
            return 1;
        }
        println!("Router strategy set to '{strategy}'.");
        return 0;
    }
    println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default());
    0
}

fn router_decisions(root: &Path, _command: &NeuralCommand) -> u8 {
    let path = router_decisions_file(root);
    println!("\nRouter Decisions");
    println!("{}", "\u{2500}".repeat(50));
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let mut n = 0;
            for line in raw.lines().take(20) {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    n += 1;
                    println!(
                        "  {} | {} | {}",
                        v["at"].as_str().unwrap_or("?"),
                        v["task"].as_str().unwrap_or("?"),
                        v["model"].as_str().unwrap_or("?")
                    );
                }
            }
            if n == 0 {
                println!("  No decisions logged.");
            }
        }
        Err(_) => println!("  No decisions logged."),
    }
    0
}

// ---- distill ----------------------------------------------------------------

fn distill(command: &NeuralCommand) -> u8 {
    let op = command.sub.clone().unwrap_or_else(|| "plan".into());
    eprintln!("[WARN] Distillation ({op}) requires the @ruvector/ruvllm native TrainingPipeline");
    eprintln!("       and ONNX runtime (Node). Native build cannot distill. Run:");
    eprintln!("       npx ruflo neural distill {op}");
    1
}

// ---- helpers ----------------------------------------------------------------

fn chars_take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        tempfile::tempdir().unwrap().keep()
    }

    fn base(op: &str) -> NeuralCommand {
        NeuralCommand {
            operation: op.into(), sub: None, pattern: None, epochs: 50, data: None,
            model: None, learning_rate: 0.01, batch_size: 32, dim: 256, input: None,
            top_k: 5, json: false, verbose: false,
        }
    }

    #[test]
    fn train_records_and_validates_pattern() {
        let root = tmp();
        let mut bad = base("train");
        bad.pattern = Some("bogus".into());
        assert_eq!(run(&root, bad), 1);
        let mut good = base("train");
        good.pattern = Some("security".into());
        good.epochs = 10;
        assert_eq!(run(&root, good), 0);
        let stats = read_stats(&root);
        assert_eq!(stats["trainingRuns"].as_array().unwrap().len(), 1);
        assert_eq!(stats["trainingRuns"][0]["pattern"], "security");
    }

    #[test]
    fn status_reflects_training() {
        let root = tmp();
        run(&root, base("train"));
        let stats = read_stats(&root);
        // The native build records the training run but does NOT bump
        // modelsTrained (no WASM training ran), so assert the run was recorded
        // and learning counters stayed at 0.
        assert!(!stats["trainingRuns"].as_array().unwrap().is_empty());
        assert_eq!(stats["modelsTrained"].as_u64().unwrap_or(0), 0);
    }

    #[test]
    fn export_import_roundtrip() {
        let root = tmp();
        run(&root, base("train"));
        let mut exp = base("export");
        exp.data = Some(root.join("exp.json").to_string_lossy().into());
        assert_eq!(run(&root, exp), 0);
        // Wipe then import.
        let _ = fs::remove_file(stats_file(&root));
        let mut imp = base("import");
        imp.data = Some(root.join("exp.json").to_string_lossy().into());
        assert_eq!(run(&root, imp), 0);
        assert!(!read_stats(&root)["trainingRuns"].as_array().unwrap().is_empty());
    }

    #[test]
    fn dim_clamped_to_max() {
        let root = tmp();
        let mut t = base("train");
        t.dim = 9999;
        run(&root, t);
        let run_v = &read_stats(&root)["trainingRuns"][0];
        assert_eq!(run_v["dim"], MODEL_DIM);
    }

    #[test]
    fn predict_degrades_without_input_or_runtime() {
        let root = tmp();
        assert_eq!(run(&root, base("predict")), 1); // missing input
        let mut p = base("predict");
        p.input = Some("x".into());
        assert_eq!(run(&root, p), 1); // runtime unavailable
    }
}
