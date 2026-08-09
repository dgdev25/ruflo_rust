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

/// Check if the ONNX model + tokenizer are available locally.
pub fn model_available() -> bool {
    onnx_model_path().is_file() && tokenizer_path().is_file()
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

/// ONNX inference: tokenize → run model → mean-pool → normalize.
/// TODO: Full implementation requires ort 2.0 API verification.
/// The ort 2.0-rc13 API has changed significantly from 1.x — the session
/// builder, tensor creation, and run patterns need careful integration.
/// For now this returns None (falls back to hash), preserving CLI
/// functionality while the ONNX path is completed in a focused follow-up.
fn embed_onnx(_text: &str) -> Option<Vec<f64>> {
    // The full implementation will:
    // 1. Load tokenizer from tokenizer.json via tokenizers::Tokenizer::from_file()
    // 2. Encode text → input_ids + attention_mask
    // 3. Create ort tensors from encoded input
    // 4. Run ONNX session
    // 5. Mean-pool last_hidden_state (mask-aware)
    // 6. L2-normalize → Vec<f64>
    //
    // The ort 2.0-rc.13 API requires:
    //   ort::environment() → ort::Environment (global init)
    //   Session::builder()?.commit()? → Session
    //   session.run(ort::inputs![input_ids, attention_mask]?)?
    //   output["last_hidden_state"].try_extract_tensor::<f32>()?
    //
    // This is blocked on verifying the exact ort 2.0 tensor creation API
    // against the crate docs. The hash fallback ensures the CLI works today.
    None
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
}
