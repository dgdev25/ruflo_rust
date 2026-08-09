//! Native V3 `embeddings` command — vector generation, similarity, hyperbolic ops.
//!
//! Source: `v3/@claude-flow/cli/src/commands/embeddings.ts`. Fifteen subcommands:
//! generate / search / compare / collections / index / init / providers / chunk
//! / normalize / hyperbolic / neural / models / cache / warmup / benchmark.
//!
//! The TS source runs an ONNX MiniLM model via `@claude-flow/transformers` for
//! real vector embeddings. ADR-0005 forbids a JS/ONNX runtime in the native
//! build, so `generate` uses a deterministic feature-hashing vectorizer
//! (per-token FNV-1a into a fixed-dim vector, L2-normalized) — a real,
//! reproducible embedding that powers `compare`, `benchmark`, and `hyperbolic`.
//! It is NOT the MiniLM model and says so. Store-backed subcommands (search /
//! collections / index) require the sql.js + HNSW index that lives in the Node
//! memory layer and degrade honestly.

use std::fs;
use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

const DEFAULT_DIM: usize = 384;
const MODEL_NAME: &str = "all-MiniLM-L6-v2 (native: deterministic feature-hash vectorizer)";

// ---- deterministic vectorizer ----------------------------------------------

/// FNV-1a 64-bit over a token's lowercased bytes.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Deterministic embedding: each whitespace/word token is hashed and mixed into
/// the dimension it maps to (sign + magnitude from a second hash), then the
/// vector is L2-normalized. Same text always yields the same vector, so
/// `compare` and `search` are reproducible. Cosine similarity between related
/// texts is higher than between unrelated ones because shared tokens contribute
/// to the same dimensions.
pub fn embed(text: &str, dim: usize) -> Vec<f64> {
    let mut v = vec![0f64; dim];
    let lower = text.to_lowercase();
    let mut token_count = 0usize;
    for token in lower.split(|c: char| c.is_whitespace() || c == '_') {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric());
        if token.is_empty() {
            continue;
        }
        token_count += 1;
        // Character trigrams give sub-word signal (helps partial matches).
        let grams: Vec<String> = if token.chars().count() <= 3 {
            vec![token.to_string()]
        } else {
            (0..token.chars().count().saturating_sub(2))
                .map(|i| token.chars().skip(i).take(3).collect())
                .collect()
        };
        for gram in grams.iter().chain(std::iter::once(&token.to_string())) {
            let h1 = fnv1a(gram) as usize;
            let h2 = fnv1a(&format!("salt{gram}"));
            let idx = h1 % dim;
            let sign = if h2 & 1 == 0 { 1.0 } else { -1.0 };
            v[idx] += sign;
        }
    }
    if token_count == 0 {
        return v;
    }
    l2_normalize(&mut v);
    v
}

fn l2_normalize(v: &mut [f64]) {
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot = a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

fn euclidean(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Poincaré ball distance. The standard formula is
///   d(x,y) = (1/√|k|) · acosh(1 + 2·‖x−y‖² / ((1−‖x‖²)(1−‖y‖²)))
/// where ‖·‖² is the squared Euclidean norm. Curvature k must be negative
/// (validated at the caller); |k| scales the result.
fn poincare_distance(a: &[f64], b: &[f64], curvature: f64) -> f64 {
    if curvature >= 0.0 {
        // Hyperbolic (Poincaré) distance is only defined for negative curvature.
        return f64::NAN;
    }
    let norm_a_sq: f64 = a.iter().map(|x| x * x).sum();
    let norm_b_sq: f64 = b.iter().map(|x| x * x).sum();
    let diff_sq: f64 = a.iter().zip(b).map(|(x, y)| x - y).map(|d| d * d).sum();
    let alpha = 1.0 - norm_a_sq;
    let beta = 1.0 - norm_b_sq;
    let denom = alpha * beta;
    if denom <= 0.0 {
        // One point is on/over the ball boundary — distance is infinite.
        return f64::INFINITY;
    }
    let arg = 1.0 + 2.0 * diff_sq / denom;
    // acosh is defined for arg >= 1; arg is always >= 1 here since diff_sq >= 0.
    let acosh = (arg + (arg * arg - 1.0).sqrt()).ln();
    acosh / curvature.abs().sqrt()
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingsCommand {
    pub operation: String,
    pub text: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub output: Option<String>,
    pub query: Option<String>,
    pub collection: Option<String>,
    pub limit: Option<usize>,
    pub threshold: Option<f64>,
    pub db_path: Option<String>,
    pub text1: Option<String>,
    pub text2: Option<String>,
    pub metric: Option<String>,
    pub action: Option<String>,
    pub name: Option<String>,
    pub dim: Option<usize>,
    pub hyperbolic: bool,
    pub curvature: Option<f64>,
    pub download: bool,
    pub cache_size: Option<usize>,
    pub ef_construction: Option<usize>,
    pub m_param: Option<usize>,
    pub json: bool,
}

pub fn run(_root: &Path, command: EmbeddingsCommand) -> u8 {
    match command.operation.as_str() {
        "" => overview(&command),
        "generate" => generate(&command),
        "search" => search(&command),
        "ingest" => ingest(&command),
        "compare" => compare(&command),
        "collections" => collections(&command),
        "index" => index_cmd(&command),
        "init" => init(&command),
        "providers" => providers(&command),
        "chunk" => chunk(&command),
        "normalize" => normalize(&command),
        "hyperbolic" => hyperbolic_cmd(&command),
        "neural" => neural(&command),
        "models" => models(&command),
        "cache" => cache_cmd(&command),
        "warmup" => warmup(&command),
        "benchmark" => benchmark(&command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (generate|search|ingest|compare|collections|index|init|providers|chunk|normalize|hyperbolic|neural|models|cache|warmup|benchmark)",
                command.operation
            );
            1
        }
    }
}

fn overview(_command: &EmbeddingsCommand) -> u8 {
    print!(r####"
RuFlo Embeddings
Vector embeddings and semantic search

Core Commands:
  - init        - Initialize ONNX models and hyperbolic config
  - generate    - Generate embeddings for text
  - search      - Semantic similarity search
  - compare     - Compare similarity between texts
  - collections - Manage embedding collections
  - index       - Manage HNSW indexes
  - providers   - List available providers

Advanced Features:
  - chunk       - Document chunking with overlap
  - normalize   - L2/L1/minmax/zscore normalization
  - hyperbolic  - Poincaré ball embeddings
  - neural      - Neural substrate (drift, memory, swarm)
  - models      - List/download ONNX models
  - cache       - Manage persistent SQLite cache

Performance:
  - HNSW indexing: 150x-12,500x faster search
  - Agentic Flow: 75x faster than Transformers.js (~3ms)
  - Persistent cache: SQLite-backed, survives restarts
  - Hyperbolic: Better hierarchical representation

Created with ❤️ by ruv.io
"####);
    0
}

// ---- generate ---------------------------------------------------------------

fn generate(command: &EmbeddingsCommand) -> u8 {
    let Some(text) = &command.text else {
        eprintln!("[ERROR] Text is required (-t)");
        return 1;
    };
    let dim = command.dim.unwrap_or(DEFAULT_DIM);
    // Reject dim 0 (modulo-by-zero panic in embed()) and absurd dims.
    if dim == 0 || dim > 4096 {
        eprintln!("[ERROR] --dim must be in 1..=4096 (got {dim})");
        return 1;
    }
    let provider = command.provider.clone().unwrap_or_else(|| "local".into());
    // Prefer ONNX MiniLM (ort crate) when the model is available; fall back to
    // the local hash vectorizer. Both are "local" — ONNX just gives real neural
    // embeddings that match the TS runtime's output exactly.
    let (vec, embed_method) = crate::onnx_embeddings::embed(text, dim);
    let model_name = if embed_method == "onnx" {
        "all-MiniLM-L6-v2 (ONNX)"
    } else {
        MODEL_NAME
    };

    let output_fmt = command.output.clone().unwrap_or_else(|| "preview".into());

    println!("\nGenerate Embedding ({embed_method})");
    println!("{}", "\u{2500}".repeat(50));

    let start = Instant::now();
    let duration = start.elapsed().as_millis();

    println!("Embedding generated in {duration}ms via {embed_method}");

    if output_fmt == "json" || command.json {
        let out = json!({
            "text": chars_take(text, 100),
            "embedding": vec,
            "dimensions": dim,
            "model": model_name,
            "provider": provider,
            "method": embed_method,
            "duration": duration,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    if output_fmt == "array" {
        let arr: Vec<String> = vec.iter().map(|v| v.to_string()).collect();
        println!("[{}]", arr.join(", "));
        return 0;
    }

    // preview
    let preview: Vec<String> = vec.iter().take(8).map(|v| format!("{:.6}", v)).collect();
    println!();
    println!("\u{256d} Result \u{256e}");
    println!("  Provider: {provider}");
    println!("  Model: {MODEL_NAME}");
    println!("  Dimensions: {dim}");
    println!("  Text: \"{}\"", chars_take(text, 40));
    println!("  Generation time: {duration}ms");
    println!();
    println!("  Vector preview (first 8 of {dim}):");
    println!("  [{}, ...]", preview.join(", "));
    0
}

// ---- search -----------------------------------------------------------------

fn search(command: &EmbeddingsCommand) -> u8 {
    let Some(query) = &command.query else {
        eprintln!("[ERROR] Query is required (-q)");
        return 1;
    };
    let db_path = command.db_path.clone().unwrap_or_else(|| ".swarm/memory.rvf".into());
    let limit = command.limit.unwrap_or(10);
    let threshold = command.threshold.unwrap_or(0.0);
    let dim = command.dim.unwrap_or(crate::onnx_embeddings::ONNX_DIM) as u16;

    let path = Path::new(&db_path);
    if !path.exists() {
        eprintln!("[ERROR] RVF store not found: {db_path}");
        eprintln!("       Run `ruflo embeddings ingest --text \"...\" --db-path {db_path}` first.");
        return 1;
    }

    let config = ruflo_storage::AgentDbFixtureConfig::new(dim);
    let store = match ruflo_storage::RvfPersistencePort::open_agentdb(path, config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ERROR] Failed to open RVF store: {e}");
            return 1;
        }
    };

    let (qvec, method) = crate::onnx_embeddings::embed(query, dim as usize);
    let qf32: Vec<f32> = qvec.iter().map(|x| *x as f32).collect();
    let matches = match store.search_agentdb(&qf32, limit) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[ERROR] RVF search failed: {e}");
            return 1;
        }
    };

    // distance → similarity (cosine distance in [0,2] → similarity in [-1,1]).
    let results: Vec<_> = matches.into_iter()
        .map(|m| {
            let sim = (1.0 - m.distance).clamp(-1.0, 1.0);
            (m.id, m.distance, sim)
        })
        .filter(|(_, _, sim)| *sim >= threshold as f32)
        .collect();

    if command.json {
        let out = serde_json::json!({
            "query": query,
            "backend": "ruvector-rvf-hnsw",
            "embedding": method,
            "results": results.iter().map(|(id, dist, sim)| serde_json::json!({
                "id": id, "distance": dist, "similarity": sim,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }

    println!("\nSemantic Search (RuVector HNSW)");
    println!("{}", "\u{2500}".repeat(50));
    println!("  Query:    \"{query}\"");
    println!("  Backend:  ruvector-rvf (HNSW)");
    println!("  Embed:    {method}");
    println!("  Limit:    {limit}, Threshold: {threshold}");
    if results.is_empty() {
        println!("  No matches above threshold.");
    } else {
        for (id, dist, sim) in &results {
            println!("  id={id:<6} dist={dist:.4} sim={sim:.4}");
        }
    }
    0
}

/// Ingest text into the RVF HNSW store. Each call embeds the text and adds it
/// as a vector with a monotonic id.
fn ingest(command: &EmbeddingsCommand) -> u8 {
    let Some(text) = &command.text else {
        eprintln!("[ERROR] --text is required for ingest");
        return 1;
    };
    let db_path = command.db_path.clone().unwrap_or_else(|| ".swarm/memory.rvf".into());
    let dim = command.dim.unwrap_or(crate::onnx_embeddings::ONNX_DIM) as u16;
    let path = Path::new(&db_path);
    // Open existing or create.
    let mut store = if path.exists() {
        let config = ruflo_storage::AgentDbFixtureConfig::new(dim);
        match ruflo_storage::RvfPersistencePort::open_agentdb(path, config) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[ERROR] Open failed, trying create: {e}");
                let config2 = ruflo_storage::AgentDbFixtureConfig::new(dim);
                match ruflo_storage::RvfPersistencePort::create_agentdb(path, config2) {
                    Ok(s) => s,
                    Err(e2) => {
                        eprintln!("[ERROR] Failed to create RVF store: {e2}");
                        return 1;
                    }
                }
            }
        }
    } else {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let config = ruflo_storage::AgentDbFixtureConfig::new(dim);
        match ruflo_storage::RvfPersistencePort::create_agentdb(path, config) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[ERROR] Failed to create RVF store: {e}");
                return 1;
            }
        }
    };

    // Determine next id from current vector count.
    let status = store.status();
    let next_id = status.total_vectors + 1;
    let (vec, method) = crate::onnx_embeddings::embed(text, dim as usize);
    let record = ruflo_storage::AgentDbVectorRecord {
        id: next_id,
        vector: vec.iter().map(|x| *x as f32).collect(),
    };
    let added = match store.ingest_agentdb(&[record]) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[ERROR] Ingest failed: {e}");
            return 1;
        }
    };
    let _ = store.close();

    if command.json {
        println!("{}", serde_json::json!({
            "ingested": added,
            "id": next_id,
            "backend": "ruvector-rvf-hnsw",
            "embedding": method,
            "path": db_path,
        }));
    } else {
        println!("\nIngest (RuVector HNSW)");
        println!("{}", "\u{2500}".repeat(50));
        println!("  Text:     \"{text}\"");
        println!("  Id:       {next_id}");
        println!("  Embed:    {method}");
        println!("  Backend:  {db_path}");
    }
    0
}

// ---- compare ----------------------------------------------------------------

fn compare(command: &EmbeddingsCommand) -> u8 {
    let (Some(t1), Some(t2)) = (&command.text1, &command.text2) else {
        eprintln!("[ERROR] --text1 and --text2 are required");
        return 1;
    };
    let metric = command.metric.clone().unwrap_or_else(|| "cosine".into());
    let dim = command.dim.unwrap_or(DEFAULT_DIM);

    println!("\nCompare Embeddings");
    println!("{}", "\u{2500}".repeat(50));

    let v1 = embed(t1, dim);
    let v2 = embed(t2, dim);
    let (score, label) = match metric.as_str() {
        "cosine" => (cosine(&v1, &v2), "Cosine similarity"),
        "euclidean" => (euclidean(&v1, &v2), "Euclidean distance"),
        "dot" => (dot(&v1, &v2), "Dot product"),
        other => {
            eprintln!("[ERROR] Unknown metric: {other} (cosine|euclidean|dot)");
            return 1;
        }
    };

    if command.json {
        let out = json!({
            "text1": t1, "text2": t2, "metric": metric, "score": score, "dimensions": dim,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("  Text 1: \"{t1}\"");
    println!("  Text 2: \"{t2}\"");
    println!("  Metric: {metric}");
    println!();
    println!("  {label}: {score:.6}");
    if metric == "cosine" {
        let pct = ((score + 1.0) / 2.0 * 100.0).clamp(0.0, 100.0);
        println!("  Similarity: {pct:.1}%");
    }
    0
}

// ---- collections ------------------------------------------------------------

fn collections(command: &EmbeddingsCommand) -> u8 {
    let action = command.action.clone().unwrap_or_else(|| "list".into());
    let db_path = command.db_path.clone().unwrap_or_else(|| ".swarm/memory.db".into());
    println!("\nCollections ({action})");
    println!("{}", "\u{2500}".repeat(50));
    if !Path::new(&db_path).exists() {
        println!("  No memory store at {db_path}.");
        println!("  Native build cannot enumerate sql.js namespaces — use a Node runtime.");
        return 0;
    }
    eprintln!("[WARN] Native collections cannot read the sql.js store. Use `npx ruflo embeddings collections`.");
    0
}

// ---- index ------------------------------------------------------------------

fn index_cmd(command: &EmbeddingsCommand) -> u8 {
    let action = command.action.clone().unwrap_or_else(|| "status".into());
    println!("\nHNSW Index ({action})");
    println!("{}", "\u{2500}".repeat(50));
    match action.as_str() {
        "status" => {
            println!("  Index: not built (native build has no HNSW runtime)");
            println!("  Build with: npx ruflo embeddings index build");
        }
        "build" | "rebuild" | "optimize" => {
            eprintln!("[WARN] HNSW index build requires the ONNX + hnswlib runtime (Node).");
            eprintln!("       Run: npx ruflo embeddings index {action}");
        }
        other => {
            eprintln!("[ERROR] Unknown index action: {other} (build|rebuild|status|optimize)");
            return 1;
        }
    }
    0
}

// ---- init -------------------------------------------------------------------

fn init(command: &EmbeddingsCommand) -> u8 {
    let model = command.model.clone().unwrap_or_else(|| "all-MiniLM-L6-v2".into());
    let hyperbolic = command.hyperbolic;
    let curvature = command.curvature.unwrap_or(-1.0);
    let cache_size = command.cache_size.unwrap_or(256);

    let dir = Path::new(".claude-flow");
    if fs::create_dir_all(dir).is_err() {
        eprintln!("[ERROR] Failed to create config dir {}", dir.display());
        return 1;
    }
    let cfg = json!({
        "model": model,
        "dimensions": DEFAULT_DIM,
        "hyperbolic": hyperbolic,
        "curvature": curvature,
        "cacheSize": cache_size,
        "nativeVectorizer": true,
        "initializedAt": now_ms(),
    });
    let path = dir.join("embeddings-config.json");
    if fs::write(&path, serde_json::to_vec_pretty(&cfg).unwrap_or_default()).is_err() {
        eprintln!("[ERROR] Failed to write config {}", path.display());
        return 1;
    }
    println!("\n\u{2714} Embeddings config written to {}", path.display());
    println!("  Model: {model} ({DEFAULT_DIM}-dim, native vectorizer)");
    println!("  Hyperbolic: {hyperbolic}, curvature: {curvature}");
    println!("  Cache size: {cache_size}");
    0
}

// ---- providers --------------------------------------------------------------

fn providers(_command: &EmbeddingsCommand) -> u8 {
    println!("\nEmbedding Providers");
    println!("{}", "\u{2500}".repeat(50));
    let rows = [
        ("local", "available", "deterministic feature-hash vectorizer (native)"),
        ("transformers", "needs runtime", "ONNX MiniLM via @claude-flow/transformers"),
        ("openai", "needs API key", "OpenAI text-embedding models"),
        ("agentic-flow", "needs runtime", "agentic-flow embedding service"),
    ];
    println!("  {:<14} {:<15} Notes", "Provider", "Status");
    println!("  {} {} {}", "\u{2500}".repeat(14), "\u{2500}".repeat(15), "\u{2500}".repeat(40));
    for (p, s, n) in rows {
        println!("  {:<14} {:<15} {}", p, s, n);
    }
    0
}

// ---- chunk ------------------------------------------------------------------

fn chunk(command: &EmbeddingsCommand) -> u8 {
    let Some(text) = &command.text else {
        eprintln!("[ERROR] Text is required (-t)");
        return 1;
    };
    let limit = command.limit.unwrap_or(200);
    println!("\nChunk Text (target ~{limit} tokens/chunk)");
    println!("{}", "\u{2500}".repeat(50));

    // Sentence-aware chunking: split on '.', '!', '?' (keeping the terminator)
    // and newlines, then accumulate up to the token limit.
    let mut sentences: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        cur.push(ch);
        if matches!(ch, '.' | '!' | '?' | '\n') {
            sentences.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur);
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0usize;
    for sent in sentences {
        let sent = sent.trim();
        if sent.is_empty() {
            continue;
        }
        let tokens = sent.split_whitespace().count();
        if current_tokens + tokens > limit && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        // If a single sentence alone exceeds the limit, hard-split it by words
        // so one oversized sentence can't blow past the budget.
        if tokens > limit {
            let words: Vec<&str> = sent.split_whitespace().collect();
            for chunk_words in words.chunks(limit.max(1)) {
                chunks.push(chunk_words.join(" "));
            }
            current_tokens = 0;
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sent);
        current_tokens += tokens;
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    if command.json {
        let out = json!({"chunks": chunks, "count": chunks.len()});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("  Chunks: {}", chunks.len());
    for (i, c) in chunks.iter().enumerate() {
        println!("\n  [{}] ({} tokens):", i + 1, c.split_whitespace().count());
        println!("    {}", chars_take(c, 80));
    }
    0
}

// ---- normalize --------------------------------------------------------------

fn normalize(command: &EmbeddingsCommand) -> u8 {
    let dim = command.dim.unwrap_or(DEFAULT_DIM);
    if let Some(text) = &command.text {
        let v = embed(text, dim); // already L2-normalized
        let arr: Vec<String> = v.iter().take(8).map(|x| format!("{:.6}", x)).collect();
        println!("\nNormalized vector (first 8 of {dim}):");
        println!("  [{}, ...]", arr.iter().take(8).map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        if command.json {
            println!("{}", serde_json::to_string_pretty(&json!({"normalized": v, "dimensions": dim})).unwrap_or_default());
        }
        return 0;
    }
    eprintln!("[ERROR] Text is required (-t)");
    1
}

// ---- hyperbolic -------------------------------------------------------------

fn hyperbolic_cmd(command: &EmbeddingsCommand) -> u8 {
    let action = command.action.clone().unwrap_or_else(|| "status".into());
    let curvature = command.curvature.unwrap_or(-1.0);
    let dim = command.dim.unwrap_or(DEFAULT_DIM);

    println!("\nHyperbolic Embeddings ({action})");
    println!("{}", "\u{2500}".repeat(50));
    println!("  Curvature: {curvature}");

    match action.as_str() {
        "status" | "convert" => {
            let Some(text) = &command.text else {
                eprintln!("[ERROR] Text is required (-t) for {action}");
                return 1;
            };
            // Convert: scale Euclidean embedding into the Poincaré ball (|x|<1).
            let eucl = embed(text, dim);
            let mut poinc: Vec<f64> = eucl.iter().map(|x| x * 0.5).collect();
            let norm: f64 = poinc.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm >= 1.0 {
                let scale = 0.99 / norm;
                for x in poinc.iter_mut() {
                    *x *= scale;
                }
            }
            if command.json {
                println!("{}", serde_json::to_string_pretty(&json!({"poincare": poinc, "curvature": curvature, "norm": norm})).unwrap_or_default());
            } else {
                let preview: Vec<String> = poinc.iter().take(6).map(|x| format!("{:.6}", x)).collect();
                println!("  Poincaré vector (first 6 of {dim}):");
                println!("  [{}, ...]", preview.join(", "));
                println!("  Norm: {norm:.6} (< 1, inside ball)");
            }
        }
        "distance" => {
            let (Some(t1), Some(t2)) = (&command.text1, &command.text2) else {
                eprintln!("[ERROR] --text1 and --text2 required for distance");
                return 1;
            };
            let scale = 0.5;
            let mut a: Vec<f64> = embed(t1, dim).iter().map(|x| x * scale).collect();
            let mut b: Vec<f64> = embed(t2, dim).iter().map(|x| x * scale).collect();
            for v in [&mut a, &mut b] {
                let n: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
                if n >= 1.0 {
                    let s = 0.99 / n;
                    for x in v.iter_mut() {
                        *x *= s;
                    }
                }
            }
            let d = poincare_distance(&a, &b, curvature);
            if command.json {
                println!("{}", serde_json::to_string_pretty(&json!({"distance": d, "curvature": curvature})).unwrap_or_default());
            } else {
                println!("  Poincaré distance: {d:.6}");
            }
        }
        "midpoint" => {
            eprintln!("[WARN] Poincaré midpoint (gyromidpoint) is approximate in the native build.");
            let (Some(t1), Some(t2)) = (&command.text1, &command.text2) else {
                eprintln!("[ERROR] --text1 and --text2 required for midpoint");
                return 1;
            };
            let a = embed(t1, dim);
            let b = embed(t2, dim);
            let mid: Vec<f64> = a.iter().zip(&b).map(|(x, y)| (x + y) * 0.25).collect();
            let n: f64 = mid.iter().map(|x| x * x).sum::<f64>().sqrt();
            println!("  Approximate midpoint norm: {n:.6}");
        }
        other => {
            eprintln!("[ERROR] Unknown hyperbolic action: {other} (status|convert|distance|midpoint)");
            return 1;
        }
    }
    0
}

// ---- neural -----------------------------------------------------------------

fn neural(command: &EmbeddingsCommand) -> u8 {
    let action = command.action.clone().unwrap_or_else(|| "status".into());
    println!("\nNeural Embedding Operations ({action})");
    println!("{}", "\u{2500}".repeat(50));
    eprintln!("[WARN] Neural embedding ops (MoE attention, drift detection) require the ONNX");
    eprintln!("       runtime. Native build reports status only. Use `npx ruflo embeddings neural {action}`.");
    0
}

// ---- models -----------------------------------------------------------------

fn models(_command: &EmbeddingsCommand) -> u8 {
    println!("\nAvailable Models");
    println!("{}", "\u{2500}".repeat(50));
    println!("  {:<28} {:<8} Status", "Model", "Dims");
    println!("  {} {} {}", "\u{2500}".repeat(28), "\u{2500}".repeat(8), "\u{2500}".repeat(20));
    let rows = [
        ("all-MiniLM-L6-v2", 384, "native vectorizer (ONNX in Node build)"),
        ("all-mpnet-base-v2", 768, "ONNX only (Node build)"),
        ("text-embedding-3-small", 1536, "OpenAI API (needs key)"),
        ("text-embedding-3-large", 3072, "OpenAI API (needs key)"),
    ];
    for (m, d, s) in rows {
        println!("  {:<28} {:<8} {}", m, d, s);
    }
    0
}

// ---- cache ------------------------------------------------------------------

fn cache_cmd(command: &EmbeddingsCommand) -> u8 {
    let action = command.action.clone().unwrap_or_else(|| "stats".into());
    let path = Path::new(".claude-flow/embeddings-cache.json");
    println!("\nEmbedding Cache ({action})");
    println!("{}", "\u{2500}".repeat(50));
    match action.as_str() {
        "stats" => {
            let stats = fs::read_to_string(path).unwrap_or_else(|_| "{}".into());
            let v: Value = serde_json::from_str(&stats).unwrap_or(json!({}));
            println!("  Entries: {}", v["entries"].as_u64().unwrap_or(0));
            println!("  Max size: {}", v["maxSize"].as_u64().unwrap_or(256));
            println!("  Hits: {}", v["hits"].as_u64().unwrap_or(0));
            println!("  Misses: {}", v["misses"].as_u64().unwrap_or(0));
        }
        "clear" => {
            let _ = fs::remove_file(path);
            println!("\u{2714} Cache cleared.");
        }
        other => {
            eprintln!("[ERROR] Unknown cache action: {other} (stats|clear)");
            return 1;
        }
    }
    0
}

// ---- warmup -----------------------------------------------------------------

fn warmup(_command: &EmbeddingsCommand) -> u8 {
    println!("\nWarming up embedding model...");
    // Eagerly compile + run the vectorizer once so the first real call is fast.
    let _ = embed("warmup", DEFAULT_DIM);
    println!("\u{2714} Native vectorizer warmed up (no ONNX model to load).");
    0
}

// ---- benchmark --------------------------------------------------------------

fn benchmark(command: &EmbeddingsCommand) -> u8 {
    let iterations = command.limit.unwrap_or(100);
    let dim = command.dim.unwrap_or(DEFAULT_DIM);
    let sample = command.text.clone().unwrap_or_else(|| {
        "The quick brown fox jumps over the lazy dog near the river bank at sunset.".into()
    });
    println!("\nEmbedding Benchmark ({iterations} iterations, {dim}-dim)");
    println!("{}", "\u{2500}".repeat(50));

    // Warmup.
    let _ = embed(&sample, dim);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = embed(&sample, dim);
    }
    let total = start.elapsed().as_secs_f64() * 1000.0;
    let per = total / iterations as f64;
    let throughput = if per > 0.0 { 1000.0 / per } else { f64::INFINITY };

    if command.json {
        let out = json!({
            "iterations": iterations, "totalMs": total, "perEmbeddingMs": per,
            "embeddingsPerSecond": throughput, "dimensions": dim,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    println!("  Total: {total:.2}ms");
    println!("  Per embedding: {per:.3}ms");
    println!("  Throughput: {throughput:.0} embeddings/sec");
    println!("  Dimensions: {dim}");
    0
}

// ---- helpers ----------------------------------------------------------------

fn chars_take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_is_deterministic() {
        let a = embed("hello world", 64);
        let b = embed("hello world", 64);
        assert_eq!(a, b);
    }

    #[test]
    fn embed_is_normalized() {
        let v = embed("some text here", 64);
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "norm = {norm}");
    }

    #[test]
    fn cosine_similarity_orders_relatedness() {
        let a = embed("the cat sat on the mat", 128);
        let b = embed("the cat sat on the mat", 128);
        let c = embed("quantum field theory equations", 128);
        assert!(cosine(&a, &b) > 0.999, "identical text must be ~1.0");
        assert!(cosine(&a, &c) < cosine(&a, &b), "unrelated text less similar");
    }

    #[test]
    fn empty_text_is_zero_vector() {
        let v = embed("", 32);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn euclidean_and_dot_consistent() {
        let a = embed("alpha", 64);
        let b = embed("alpha", 64);
        assert!(euclidean(&a, &b) < 1e-9);
        // normalized unit vectors => dot == cosine
        assert!((dot(&a, &b) - cosine(&a, &b)).abs() < 1e-9);
    }

    #[test]
    fn poincare_distance_zero_for_identical() {
        let a = embed("x", 32).iter().map(|x| x * 0.5).collect::<Vec<_>>();
        assert!(poincare_distance(&a, &a, -1.0) < 1e-9);
    }

    #[test]
    fn fnv1a_known_vector() {
        // FNV-1a 64 of empty string is the offset basis.
        assert_eq!(fnv1a(""), 0xcbf29ce484222325);
    }
}
