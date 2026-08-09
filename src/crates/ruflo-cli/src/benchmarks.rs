//! Benchmark suites — ports benchmarks/ evaluation harness state.
//!
//! The TS benchmarks/ (9.4K LOC) contains GAIA benchmark implementations
//! (decompose, judge, vote, extract) that require LLM calls. Those need a
//! provider adapter and are deferred. This module ports the RESULT RECORDING +
//! COMPARISON layer: saves benchmark runs to state files, compares runs,
//! generates summary reports. Also ports the GAIA evaluation scoring + smoke
//! test structure (state-level).

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn bench_dir() -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".claude-flow/benchmarks")
}

fn read_results(name: &str) -> Value {
    std::fs::read_to_string(bench_dir().join(format!("{name}.json")))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"runs": []}))
}

fn write_results(name: &str, v: &Value) -> bool {
    let dir = bench_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.json"));
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    std::fs::write(&path, &bytes).is_ok()
}

/// Record a benchmark run result.
pub fn record_run(suite: &str, metrics: Value) -> Value {
    let mut state = read_results(suite);
    let run = json!({
        "id": format!("run-{}", now_ms()),
        "suite": suite,
        "metrics": metrics,
        "at": now_ms(),
    });
    if state["runs"].is_null() { state["runs"] = json!([]); }
    state["runs"].as_array_mut().unwrap().push(run.clone());
    state["totalRuns"] = json!(state["runs"].as_array().map(|a| a.len()).unwrap_or(0));
    write_results(suite, &state);
    run
}

/// Get the latest run for a suite.
pub fn latest_run(suite: &str) -> Option<Value> {
    let state = read_results(suite);
    state["runs"].as_array()?.last().cloned()
}

/// Compare two runs by suite + run IDs.
pub fn compare_runs(suite: &str, run_a: &str, run_b: &str) -> Value {
    let state = read_results(suite);
    let runs = state["runs"].as_array().cloned().unwrap_or_default();
    let a = runs.iter().find(|r| r["id"].as_str() == Some(run_a)).cloned();
    let b = runs.iter().find(|r| r["id"].as_str() == Some(run_b)).cloned();
    json!({
        "suite": suite,
        "runA": a,
        "runB": b,
        "comparedAt": now_ms(),
    })
}

/// Generate a summary report across all suites.
pub fn summary_report() -> Value {
    let mut summaries = Vec::new();
    for suite in &["pretrain", "neural", "memory", "all", "gaia"] {
        let state = read_results(suite);
        let runs = state["runs"].as_array().cloned().unwrap_or_default();
        if !runs.is_empty() {
            summaries.push(json!({
                "suite": suite,
                "totalRuns": runs.len(),
                "latestAt": runs.last().and_then(|r| r["at"].as_u64()).unwrap_or(0),
            }));
        }
    }
    json!({"suites": summaries, "generatedAt": now_ms()})
}

// ---- GAIA evaluation scoring ----

/// Score a GAIA evaluation: compare agent answer against ground truth.
/// Ports the scoring logic from gaia-judge.ts (without the LLM judge call).
pub fn score_gaia(predictions: &[(String, String)]) -> Value {
    let total = predictions.len();
    let exact_matches = predictions.iter()
        .filter(|(pred, truth)| pred.trim().eq_ignore_ascii_case(truth.trim()))
        .count();
    let partial = predictions.iter()
        .filter(|(pred, truth)| {
            !pred.trim().eq_ignore_ascii_case(truth.trim())
                && (pred.to_lowercase().contains(&truth.to_lowercase())
                    || truth.to_lowercase().contains(&pred.to_lowercase()))
        })
        .count();
    let accuracy = if total > 0 { exact_matches as f64 / total as f64 } else { 0.0 };
    json!({
        "total": total,
        "exactMatches": exact_matches,
        "partialMatches": partial,
        "accuracy": accuracy,
        "scoredAt": now_ms(),
    })
}

/// GAIA smoke test structure — a minimal test that exercises the scoring path
/// without needing the full dataset.
pub fn gaia_smoke() -> Value {
    let smoke_cases = vec![
        ("Paris".into(), "Paris".into()),          // exact match
        ("London, UK".into(), "London".into()),     // partial
        ("42".into(), "42".into()),                 // exact
        ("wrong".into(), "correct".into()),         // miss
    ];
    let score = score_gaia(&smoke_cases);
    json!({
        "type": "smoke",
        "cases": smoke_cases.len(),
        "score": score,
        "smokeAt": now_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn record_and_latest() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        record_run("test", json!({"latency_ms": 42}));
        record_run("test", json!({"latency_ms": 38}));
        let latest = latest_run("test").unwrap();
        assert_eq!(latest["metrics"]["latency_ms"], 38);
        let state = read_results("test");
        assert_eq!(state["totalRuns"], 2);
    }

    #[test]
    fn gaia_scoring() {
        let preds = vec![
            ("Paris".into(), "Paris".into()),
            ("wrong".into(), "right".into()),
        ];
        let score = score_gaia(&preds);
        assert_eq!(score["total"], 2);
        assert_eq!(score["exactMatches"], 1);
        assert!((score["accuracy"].as_f64().unwrap() - 0.5).abs() < 0.01);
    }

    #[test]
    fn gaia_smoke_runs() {
        let smoke = gaia_smoke();
        assert_eq!(smoke["cases"], 4);
        assert_eq!(smoke["score"]["exactMatches"], 2); // Paris + 42
    }

    #[test]
    fn summary_works() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        record_run("pretrain", json!({"score": 0.85}));
        let report = summary_report();
        assert!(report["suites"].as_array().unwrap().len() >= 1);
    }
}
