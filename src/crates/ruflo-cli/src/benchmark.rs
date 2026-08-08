//! Native V3 `benchmark` command — performance benchmarking suite.
//!
//! Source: `v3/@claude-flow/cli/src/commands/benchmark.ts`. Four suites:
//! pretrain/neural/memory/all. When the benchmark runtime is unavailable the
//! command returns structured null metrics (handoff: "never fake measurements").

use std::path::Path;

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkCommand {
    Overview,
    Pretrain {
        iterations: usize,
        warmup: usize,
        output: String,
        save: Option<String>,
    },
    Neural {
        iterations: usize,
        dimension: usize,
        vectors: usize,
        output: String,
        save: Option<String>,
    },
    Memory {
        iterations: usize,
        output: String,
        save: Option<String>,
    },
    All {
        iterations: usize,
        output: String,
        save: Option<String>,
    },
    Help {
        subcommand: Option<String>,
    },
}

pub fn run(_root: &Path, command: BenchmarkCommand) -> u8 {
    match command {
        BenchmarkCommand::Overview => {
            print!("{OVERVIEW}");
            0
        }
        BenchmarkCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
        BenchmarkCommand::Pretrain {
            iterations,
            warmup,
            output,
            save,
        } => run_suite("pretrain", &output, save, |_| {
            json!({
                "suite": "pretrain",
                "iterations": iterations,
                "warmup": warmup,
                "results": null,
                "note": "Pre-training benchmark runtime (SONA/EWC++/MoE) not available in native build."
            })
        }),
        BenchmarkCommand::Neural {
            iterations,
            dimension,
            vectors,
            output,
            save,
        } => run_suite("neural", &output, save, |_| {
            json!({
                "suite": "neural",
                "iterations": iterations,
                "config": {"dimension": dimension, "vectors": vectors},
                "results": null,
                "note": "Neural benchmark runtime (ONNX/WASM/HNSW) not available in native build."
            })
        }),
        BenchmarkCommand::Memory {
            iterations,
            output,
            save,
        } => run_suite("memory", &output, save, |_| {
            json!({
                "suite": "memory",
                "iterations": iterations,
                "results": null,
                "note": "Memory benchmark runtime (HNSW/AgentDB) not available in native build."
            })
        }),
        BenchmarkCommand::All {
            iterations,
            output,
            save,
        } => {
            let suites = ["pretrain", "neural", "memory"];
            let mut results = Vec::new();
            for suite in &suites {
                let r = json!({
                    "suite": suite,
                    "iterations": iterations,
                    "results": null,
                    "note": "Benchmark runtime not available in native build."
                });
                results.push(r);
            }
            let data = json!({"suite": "all", "iterations": iterations, "suites": results});
            print_result(&data, &output);
            if let Some(path) = &save {
                let _ = std::fs::write(
                    path,
                    serde_json::to_string_pretty(&data).unwrap_or_default(),
                );
            }
            0
        }
    }
}

fn run_suite(name: &str, output: &str, save: Option<String>, build: impl Fn(&str) -> Value) -> u8 {
    let data = build(name);
    print_result(&data, output);
    if let Some(path) = save {
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&data).unwrap_or_default(),
        );
    }
    0
}

fn print_result(data: &Value, format: &str) {
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
    } else {
        println!();
        println!(
            "Benchmark: {}",
            data.get("suite").and_then(Value::as_str).unwrap_or("?")
        );
        let iterations = data.get("iterations").and_then(Value::as_u64).unwrap_or(0);
        println!("Iterations: {iterations}");
        if let Some(note) = data.get("note").and_then(Value::as_str) {
            println!("Status: degraded — {note}");
        }
        let results = data.get("results");
        if results.is_some() && !results.map(|r| r.is_null()).unwrap_or(true) {
            println!("Results: {}", results.unwrap());
        } else {
            println!("Results: (not available)");
        }
    }
}

fn help(sub: Option<&str>) -> &'static str {
    match sub {
        Some("pretrain") => "\nruflo benchmark pretrain\nBenchmark self-learning pre-training (SONA, EWC++, MoE)\n\nOPTIONS:\n  -i, --iterations <number>  Benchmark iterations [default: 100]\n  -w, --warmup <number>      Warmup iterations [default: 10]\n  -o, --output <value>       Output format: text, json [default: text]\n  -s, --save <value>         Save results to file\n",
        Some("neural") => "\nruflo benchmark neural\nBenchmark neural operations (embeddings, WASM)\n\nOPTIONS:\n  -i, --iterations <number>  Benchmark iterations [default: 100]\n  -d, --dimension <number>   Embedding dimension [default: 384]\n  -n, --vectors <number>     Number of test vectors [default: 1000]\n  -o, --output <value>       Output format: text, json [default: text]\n",
        Some("memory") => "\nruflo benchmark memory\nBenchmark memory operations (HNSW, store, search)\n\nOPTIONS:\n  -i, --iterations <number>  Benchmark iterations [default: 100]\n  -o, --output <value>       Output format: text, json [default: text]\n",
        Some("all") => "\nruflo benchmark all\nRun all benchmark suites\n\nOPTIONS:\n  -i, --iterations <number>  Benchmark iterations [default: 50]\n  -o, --output <value>       Output format: text, json [default: text]\n  -s, --save <value>         Save results to file\n",
        _ => "\nruflo benchmark\nPerformance benchmarking for self-learning and neural systems\n\nSUBCOMMANDS:\n  pretrain  Benchmark self-learning pre-training (SONA, EWC++, MoE)\n  neural    Benchmark neural operations (embeddings, WASM)\n  memory    Benchmark memory operations (HNSW, store, search)\n  all       Run all benchmark suites\n",
    }
}

const OVERVIEW: &str = r####"
RuFlo V3 Benchmark Suite
──────────────────────────────────────────────────

Available subcommands:
  pretrain  - Benchmark self-learning pre-training (SONA, EWC++, MoE)
  neural    - Benchmark neural operations (embeddings, WASM)
  memory    - Benchmark memory operations (HNSW, store, search)
  all       - Run all benchmark suites

Examples:
  claude-flow benchmark pretrain -i 200
  claude-flow benchmark all --save results.json

"####;
