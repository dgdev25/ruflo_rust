//! ONNX MiniLM embeddings — real neural inference via `ort` crate.
//!
//! Replaces the deterministic hash vectorizer with actual all-MiniLM-L6-v2
//! inference when the ONNX model is available locally. Falls back to the hash
//! vectorizer when the model is not downloaded (preserving CLI functionality).
//!
//! Model path: ~/.cache/ruflo/models/all-MiniLM-L6-v2.onnx
//! Tokenizer: ~/.cache/ruflo/models/tokenizer.json
//! Download: `ruflo embeddings init --download` (pulls from HuggingFace Hub)

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{json, Value};

/// Default embedding dimension for all-MiniLM-L6-v2.
pub const ONNX_DIM: usize = 384;
/// Node V3's default semantic embedder: Xenova/bge-base-en-v1.5.
pub const BGE_DIM: usize = 768;
/// BAAI's required query-side prefix, applied to queries only.
pub const BGE_QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";
/// Max sequence length the MiniLM tokenizer pads/truncates to.
const MAX_SEQ_LEN: usize = 128;

/// Model directory under the user's cache.
fn model_dir() -> PathBuf {
    std::env::var("RUFLO_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache/ruflo/models"))
                .unwrap_or_else(|_| PathBuf::from(".cache/ruflo/models"))
        })
}

fn onnx_model_path() -> PathBuf {
    model_dir().join("all-MiniLM-L6-v2.onnx")
}

fn tokenizer_path() -> PathBuf {
    model_dir().join("tokenizer.json")
}

fn bge_model_path() -> PathBuf {
    std::env::var("RUFLO_BGE_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| model_dir())
        .join("bge-base-en-v1.5.onnx")
}

fn bge_tokenizer_path() -> PathBuf {
    std::env::var("RUFLO_BGE_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| model_dir())
        .join("bge-base-en-v1.5-tokenizer.json")
}

/// Check if the ONNX model + tokenizer are available locally.
pub fn model_available() -> bool {
    onnx_model_path().is_file() && tokenizer_path().is_file()
}

/// BGE is opt-in by assets, never by a hash fallback. This prevents semantic
/// indexes from mixing Node-compatible BGE vectors with deterministic hashes.
pub fn bge_model_available() -> bool {
    bge_model_path().is_file() && bge_tokenizer_path().is_file()
}

pub fn embed_bge_document(text: &str) -> Option<Vec<f64>> {
    embed_bge(text)
}

pub fn embed_bge_query(text: &str) -> Option<Vec<f64>> {
    embed_bge(&format!("{BGE_QUERY_PREFIX}{text}"))
}

/// Generate embeddings using ONNX MiniLM if available, else hash fallback.
/// Returns (vector, method) where method is "onnx" or "hash".
pub fn embed(text: &str, dim: usize) -> (Vec<f64>, &'static str) {
    if model_available() {
        match embed_onnx(text) {
            Some(v) => return (v, "onnx"),
            None => {}
        }
    }
    (embed_hash(text, dim), "hash")
}

/// ONNX inference: tokenize → run model → mask-aware mean-pool → L2-normalize.
///
/// Loads the all-MiniLM-L6-v2 ONNX session + tokenizer once (cached in a
/// process-wide OnceLock), encodes the text to input_ids + attention_mask,
/// runs the model, mean-pools last_hidden_state weighted by the attention
/// mask, and L2-normalizes. Returns None only if the model isn't loaded or
/// inference fails (caller falls back to the hash vectorizer).
fn embed_onnx(text: &str) -> Option<Vec<f64>> {
    let ctx = ONNX_CTX.get_or_init(|| OnnxCtx::load().ok());
    let ctx = ctx.as_ref()?;

    // 1. Tokenize.
    let encoding = ctx.tokenizer.encode(text, true).ok()?;
    let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
    let attention_mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
    let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();
    let seq_len = input_ids.len();
    if seq_len == 0 {
        return Some(vec![0.0; ONNX_DIM]);
    }

    // 2. Build input tensors [1, seq_len] (pad/truncate to MAX_SEQ_LEN).
    let (input_ids, attention_mask, token_type_ids) = pad_to(
        input_ids, attention_mask, token_type_ids, MAX_SEQ_LEN,
    );
    let shape = vec![1_i64, MAX_SEQ_LEN as i64];

    let ids_tensor = ort::value::Tensor::from_array((shape.clone(), input_ids)).ok()?;
    let mask_tensor = ort::value::Tensor::from_array((shape.clone(), attention_mask)).ok()?;
    let type_tensor = ort::value::Tensor::from_array((shape, token_type_ids)).ok()?;

    // 3. Run the session.
    let mut session = ctx.session.lock().ok()?;
    let outputs = session.run(ort::inputs![ids_tensor, mask_tensor, type_tensor]).ok()?;

    // 4. Extract last_hidden_state [1, seq_len, hidden].
    let hidden = outputs[0].try_extract_tensor::<f32>().ok()?;
    let (_shape, data) = hidden; // (Shape, &[f32])
    // hidden_dim = total / MAX_SEQ_LEN.
    let hidden_dim = data.len() / MAX_SEQ_LEN;
    if hidden_dim == 0 {
        return None;
    }

    // 5. Mask-aware mean-pool over the real tokens, then L2-normalize.
    let mut pooled = vec![0f64; hidden_dim];
    let mut token_count = 0u32;
    for t in 0..MAX_SEQ_LEN {
        // Attention mask is padded-truncated form; recover real mask length
        // from the original encoding (token_count = original seq_len).
        if t >= seq_len {
            break;
        }
        let offset = t * hidden_dim;
        for d in 0..hidden_dim {
            pooled[d] += data[offset + d] as f64;
        }
        token_count += 1;
    }
    if token_count > 0 {
        let n = token_count as f64;
        for v in pooled.iter_mut() {
            *v /= n;
        }
    }
    let norm = pooled.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 0.0 {
        for v in pooled.iter_mut() {
            *v /= norm;
        }
    }
    Some(pooled)
}

/// Pad (or truncate) the three parallel token vectors to a fixed length so the
/// ONNX model gets a rectangular batch.
fn pad_to(
    mut ids: Vec<i64>,
    mut mask: Vec<i64>,
    mut types: Vec<i64>,
    len: usize,
) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
    if ids.len() > len {
        ids.truncate(len);
        mask.truncate(len);
        types.truncate(len);
    } else {
        let pad = len - ids.len();
        ids.resize(len, 0);
        mask.resize(len, 0); // padding tokens get attention_mask 0
        types.resize(len, 0);
        let _ = pad;
    }
    (ids, mask, types)
}

/// Cached ONNX context: tokenizer + a Mutex-guarded session (run() takes &mut).
struct OnnxCtx {
    tokenizer: tokenizers::Tokenizer,
    session: std::sync::Mutex<ort::session::Session>,
}

impl OnnxCtx {
    fn load() -> Result<Self, String> {
        let model_path = onnx_model_path();
        let tokenizer_path = tokenizer_path();
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("tokenizer load: {e}"))?;
        // Pad/truncate to MAX_SEQ_LEN so every input is rectangular.
        let mut tokenizer = tokenizer;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams {
            max_length: MAX_SEQ_LEN,
            ..Default::default()
        }))
        .map_err(|e| format!("truncation: {e}"))?;
        tokenizer.with_padding(Some(tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::Fixed(MAX_SEQ_LEN),
            ..Default::default()
        }));
        let session = ort::session::Session::builder()
            .map_err(|e| format!("session builder: {e}"))?
            .commit_from_file(&model_path)
            .map_err(|e| format!("session commit: {e}"))?;
        Ok(Self {
            tokenizer,
            session: std::sync::Mutex::new(session),
        })
    }
}

static ONNX_CTX: OnceLock<Option<OnnxCtx>> = OnceLock::new();

/// Dedicated BGE context. It deliberately has separate paths and a 512-token
/// window so installing MiniLM cannot accidentally change BGE retrieval.
struct BgeCtx {
    tokenizer: tokenizers::Tokenizer,
    session: std::sync::Mutex<ort::session::Session>,
}

impl BgeCtx {
    fn load() -> Result<Self, String> {
        let tokenizer = tokenizers::Tokenizer::from_file(bge_tokenizer_path())
            .map_err(|error| format!("BGE tokenizer load: {error}"))?;
        let mut tokenizer = tokenizer;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }))
            .map_err(|error| format!("BGE truncation: {error}"))?;
        tokenizer.with_padding(Some(tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::Fixed(512),
            ..Default::default()
        }));
        let session = ort::session::Session::builder()
            .map_err(|error| format!("BGE session builder: {error}"))?
            .commit_from_file(bge_model_path())
            .map_err(|error| format!("BGE session commit: {error}"))?;
        Ok(Self {
            tokenizer,
            session: std::sync::Mutex::new(session),
        })
    }
}

static BGE_CTX: OnceLock<Option<BgeCtx>> = OnceLock::new();

/// Exact Node policy: CLS pooling (token zero) followed by L2 normalisation.
fn embed_bge(text: &str) -> Option<Vec<f64>> {
    if !bge_model_available() {
        return None;
    }
    let context = BGE_CTX.get_or_init(|| BgeCtx::load().ok());
    let context = context.as_ref()?;
    let encoding = context.tokenizer.encode(text, true).ok()?;
    let (input_ids, attention_mask, token_type_ids) = pad_to(
        encoding.get_ids().iter().map(|id| i64::from(*id)).collect(),
        encoding
            .get_attention_mask()
            .iter()
            .map(|mask| i64::from(*mask))
            .collect(),
        encoding
            .get_type_ids()
            .iter()
            .map(|kind| i64::from(*kind))
            .collect(),
        512,
    );
    let sequence_length = 512;
    let shape = vec![1_i64, sequence_length as i64];
    let ids = ort::value::Tensor::from_array((shape.clone(), input_ids)).ok()?;
    let mask = ort::value::Tensor::from_array((shape.clone(), attention_mask)).ok()?;
    let types = ort::value::Tensor::from_array((shape, token_type_ids)).ok()?;
    let mut session = context.session.lock().ok()?;
    let outputs = session.run(ort::inputs![ids, mask, types]).ok()?;
    let (_, hidden) = outputs[0].try_extract_tensor::<f32>().ok()?;
    let dimensions = hidden.len() / sequence_length;
    if dimensions != BGE_DIM {
        return None;
    }
    let mut vector = hidden[..dimensions].iter().map(|value| f64::from(*value)).collect::<Vec<_>>();
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm > 1e-9 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    Some(vector)
}

/// Deterministic hash vectorizer (fallback when ONNX model unavailable).
/// Same FNV-1a + char-trigram algorithm as embeddings.rs.
fn embed_hash(text: &str, dim: usize) -> Vec<f64> {
    let mut v = vec![0f64; dim];
    let lower = text.to_lowercase();
    for token in lower.split(|c: char| c.is_whitespace() || c == '_') {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric());
        if token.is_empty() {
            continue;
        }
        let grams: Vec<String> = if token.chars().count() <= 3 {
            vec![token.to_string()]
        } else {
            (0..token.chars().count().saturating_sub(2))
                .map(|i| token.chars().skip(i).take(3).collect())
                .collect()
        };
        for gram in grams.iter().chain(std::iter::once(&token.to_string())) {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in gram.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            let h2 = {
                let mut h = 0xcbf29ce484222325;
                for b in format!("salt{gram}").as_bytes() {
                    h ^= *b as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
                h
            };
            let idx = h as usize % dim;
            let sign = if h2 & 1 == 0 { 1.0 } else { -1.0 };
            v[idx] += sign;
        }
    }
    // L2-normalize
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

/// Download the ONNX model from HuggingFace Hub.
/// Uses direct HTTP (reqwest-free; std::process::Command curl).
pub fn download_model() -> Result<PathBuf, String> {
    let dir = model_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let model_url = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model_quantized.onnx";
    let tokenizer_url = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

    let model_path = onnx_model_path();
    let tokenizer_path = tokenizer_path();

    // Download model (~45MB quantized).
    if !model_path.exists() {
        let status = std::process::Command::new("curl")
            .args(["-L", "-o", model_path.to_str().unwrap(), model_url])
            .output()
            .map_err(|e| format!("curl model: {e}"))?;
        if !status.status.success() {
            return Err("Failed to download ONNX model".into());
        }
    }

    // Download tokenizer (~500KB).
    if !tokenizer_path.exists() {
        let status = std::process::Command::new("curl")
            .args(["-L", "-o", tokenizer_path.to_str().unwrap(), tokenizer_url])
            .output()
            .map_err(|e| format!("curl tokenizer: {e}"))?;
        if !status.status.success() {
            return Err("Failed to download tokenizer".into());
        }
    }

    Ok(model_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_fallback_works() {
        let (v, method) = embed("hello world", 384);
        assert_eq!(method, "hash"); // ONNX model not downloaded in CI
        assert_eq!(v.len(), 384);
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn deterministic_output() {
        let (v1, _) = embed("hello world", 64);
        let (v2, _) = embed("hello world", 64);
        assert_eq!(v1, v2);
    }

    #[test]
    fn empty_text_zero_vector() {
        let (v, _) = embed("", 32);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn bge_matches_xenova_cls_pooling_reference_when_assets_are_available() {
        if !bge_model_available() {
            return;
        }
        // Reference captured from the Node V3 @xenova/transformers path with
        // the same quantized Xenova/bge-base-en-v1.5 ONNX artifact.
        let document = embed_bge_document("rotate API authentication token").unwrap();
        let query = embed_bge_query("authentication token").unwrap();
        let expected_document = [
            0.02199567937384114,
            -0.020602824455691147,
            0.03581153962083376,
            -0.02439804083473037,
            -0.02094790545603363,
        ];
        let expected_query = [
            0.03422781764041158,
            -0.017703155007118998,
            0.006269114715657386,
            0.012931262797510059,
            0.022532590336796863,
        ];
        for (actual, expected) in document.iter().zip(expected_document) {
            assert!(
                // transformers.js runs the quantized graph through its WASM
                // runtime while native Ruflo uses ONNX Runtime. Their floating
                // point kernels differ slightly; the tolerance catches model,
                // tokenizer, pooling, and query-prefix mismatches.
                (actual - expected).abs() < 1e-2,
                "document BGE mismatch: {actual} vs {expected}"
            );
        }
        for (actual, expected) in query.iter().zip(expected_query) {
            assert!(
                (actual - expected).abs() < 1e-2,
                "query BGE mismatch: {actual} vs {expected}"
            );
        }
        assert_eq!(document.len(), BGE_DIM);
        assert_eq!(query.len(), BGE_DIM);
    }
}
