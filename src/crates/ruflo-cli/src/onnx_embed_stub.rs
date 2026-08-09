//! Fallback embedding when the `onnx` feature is disabled (e.g. Windows GNU
//! cross-compile, where ort ships no prebuilt binaries). Provides the same
//! public API as onnx_embeddings.rs using the hash vectorizer only.

use serde_json::{json, Value};

pub const ONNX_DIM: usize = 384;

pub fn model_available() -> bool { false }

pub fn embed(text: &str, dim: usize) -> (Vec<f64>, &'static str) {
    (hash_embed(text, dim), "hash")
}

fn hash_embed(text: &str, dim: usize) -> Vec<f64> {
    let mut v = vec![0f64; dim];
    let lower = text.to_lowercase();
    for token in lower.split(|c: char| c.is_whitespace() || c == '_') {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric());
        if token.is_empty() { continue; }
        let grams: Vec<String> = if token.chars().count() <= 3 {
            vec![token.to_string()]
        } else {
            (0..token.chars().count().saturating_sub(2))
                .map(|i| token.chars().skip(i).take(3).collect())
                .collect()
        };
        for gram in grams.iter().chain(std::iter::once(&token.to_string())) {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in gram.as_bytes() { h ^= *b as u64; h = h.wrapping_mul(0x100000001b3); }
            let mut h2: u64 = 0xcbf29ce484222325;
            for b in format!("salt{gram}").as_bytes() { h2 ^= *b as u64; h2 = h2.wrapping_mul(0x100000001b3); }
            let idx = h as usize % dim;
            v[idx] += if h2 & 1 == 0 { 1.0 } else { -1.0 };
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 0.0 { for x in v.iter_mut() { *x /= norm; } }
    v
}
