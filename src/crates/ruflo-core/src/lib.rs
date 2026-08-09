//! Process-independent Ruflo operations.
//!
//! This crate intentionally has no argv parsing, stdout, current-directory
//! lookup, or Node runtime dependency. CLI and N-API are adapters over these
//! typed contracts.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const DEFAULT_EMBEDDING_DIMENSIONS: usize = 384;
pub const MAX_EMBEDDING_DIMENSIONS: usize = 4096;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError(String);

impl CoreError {
    fn invalid(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CoreError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedRequest {
    pub text: String,
    pub dimensions: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbedResponse {
    pub dimensions: usize,
    pub vector: Vec<f64>,
    /// The vectorizer is deterministic and suitable for portable exact
    /// matching. It is not a claim of BGE/MiniLM semantic-model equivalence.
    pub provider: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRequest {
    pub task: String,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteResponse {
    pub agent: String,
    pub score: u32,
    pub strategy: &'static str,
}

pub fn embed(request: EmbedRequest) -> Result<EmbedResponse, CoreError> {
    validate_text(&request.text, "text")?;
    let dimensions = request.dimensions.unwrap_or(DEFAULT_EMBEDDING_DIMENSIONS);
    if !(1..=MAX_EMBEDDING_DIMENSIONS).contains(&dimensions) {
        return Err(CoreError::invalid(format!(
            "dimensions must be between 1 and {MAX_EMBEDDING_DIMENSIONS}"
        )));
    }
    let mut vector = vec![0.0; dimensions];
    for token in tokens(&request.text) {
        add_feature(&mut vector, &token);
        let chars: Vec<_> = token.chars().collect();
        for index in 0..chars.len().saturating_sub(2) {
            add_feature(
                &mut vector,
                &chars[index..index + 3].iter().collect::<String>(),
            );
        }
    }
    normalize(&mut vector);
    Ok(EmbedResponse {
        dimensions,
        vector,
        provider: "deterministic-feature-hash-v1",
    })
}

pub fn cosine_similarity(left: &[f64], right: &[f64]) -> Result<f64, CoreError> {
    if left.is_empty() || left.len() != right.len() {
        return Err(CoreError::invalid(
            "vectors must be non-empty and have equal dimensions",
        ));
    }
    if left.iter().chain(right).any(|value| !value.is_finite()) {
        return Err(CoreError::invalid(
            "vectors must contain only finite values",
        ));
    }
    let left_norm = left.iter().map(|value| value * value).sum::<f64>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return Ok(0.0);
    }
    Ok(left.iter().zip(right).map(|(a, b)| a * b).sum::<f64>() / (left_norm * right_norm))
}

pub fn route(request: RouteRequest) -> Result<RouteResponse, CoreError> {
    validate_text(&request.task, "task")?;
    if request.candidates.is_empty() {
        return Err(CoreError::invalid(
            "at least one routing candidate is required",
        ));
    }
    let task = request.task.to_ascii_lowercase();
    let mut best: Option<(String, u32)> = None;
    for raw_candidate in request.candidates {
        let candidate = raw_candidate.trim();
        if candidate.is_empty() || candidate.len() > 128 {
            return Err(CoreError::invalid(
                "routing candidates must be 1..=128 bytes",
            ));
        }
        let score = route_score(&task, &candidate.to_ascii_lowercase());
        match &best {
            None => best = Some((candidate.to_owned(), score)),
            Some((name, current))
                if score > *current || (score == *current && candidate < name) =>
            {
                best = Some((candidate.to_owned(), score))
            }
            _ => {}
        }
    }
    let (agent, score) = best.expect("non-empty candidates are enforced");
    Ok(RouteResponse {
        agent,
        score,
        strategy: "deterministic-keyword-v1",
    })
}

fn validate_text(text: &str, field: &str) -> Result<(), CoreError> {
    if text.trim().is_empty() {
        return Err(CoreError::invalid(format!("{field} must not be empty")));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(CoreError::invalid(format!(
            "{field} exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn tokens(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn add_feature(vector: &mut [f64], feature: &str) {
    let first = fnv1a(feature);
    let second = fnv1a(&format!("ruflo:{feature}"));
    let index = (first as usize) % vector.len();
    vector[index] += if second & 1 == 0 { 1.0 } else { -1.0 };
}

fn fnv1a(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn normalize(vector: &mut [f64]) {
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

fn route_score(task: &str, candidate: &str) -> u32 {
    let terms: &[(&str, &[&str])] = &[
        ("coder", &["code", "implement", "refactor", "feature"]),
        ("tester", &["test", "quality", "coverage", "validate"]),
        ("reviewer", &["review", "security", "audit"]),
        ("architect", &["design", "architecture", "plan"]),
        ("researcher", &["research", "investigate", "document"]),
        ("optimizer", &["optimize", "performance", "benchmark"]),
        ("debugger", &["bug", "debug", "fix"]),
    ];
    terms
        .iter()
        .find(|(agent, _)| *agent == candidate)
        .map_or(0, |(_, keywords)| {
            keywords.iter().filter(|word| task.contains(**word)).count() as u32
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedding_is_deterministic_and_normalized() {
        let first = embed(EmbedRequest {
            text: "native addon contract".into(),
            dimensions: Some(32),
        })
        .unwrap();
        assert_eq!(
            first,
            embed(EmbedRequest {
                text: "native addon contract".into(),
                dimensions: Some(32)
            })
            .unwrap()
        );
        assert!((first.vector.iter().map(|value| value * value).sum::<f64>() - 1.0).abs() < 1e-12);
    }
    #[test]
    fn similarity_rejects_mismatched_vectors() {
        assert!(cosine_similarity(&[1.0], &[1.0, 2.0]).is_err());
    }
    #[test]
    fn routing_is_deterministic_and_prefers_matching_candidate() {
        let answer = route(RouteRequest {
            task: "optimise benchmark performance".into(),
            candidates: vec!["coder".into(), "optimizer".into()],
        })
        .unwrap();
        assert_eq!(answer.agent, "optimizer");
    }
}
