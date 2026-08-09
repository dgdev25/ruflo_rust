//! Source-compatible hybrid ranking primitives.
//!
//! Ported from `v3/@claude-flow/cli/src/memory/hybrid-retrieval.ts`: lexical
//! BM25 supplements dense cosine, then MMR removes near-duplicate results.

use std::collections::{HashMap, HashSet};

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "if", "then", "else", "of", "in", "to", "for",
    "on", "at", "by", "with", "from", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "should", "could", "can",
    "may", "might", "must", "this", "that", "these", "those", "it", "its", "as", "also",
    "not", "no", "so", "too", "very",
];

#[derive(Debug, Clone)]
pub struct CorpusStats {
    idf: HashMap<String, f64>,
    average_document_length: f64,
}

pub fn tokenize(text: &str) -> Vec<String> {
    let stopwords: HashSet<&str> = STOPWORDS.iter().copied().collect();
    text.to_ascii_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.')))
        .filter(|token| token.len() >= 3 && !stopwords.contains(*token))
        .map(ToOwned::to_owned)
        .collect()
}

pub fn build_corpus_stats(documents: &[Vec<String>]) -> CorpusStats {
    let mut document_frequency = HashMap::<String, usize>::new();
    let total_length = documents.iter().map(Vec::len).sum::<usize>();
    for document in documents {
        let unique = document.iter().collect::<HashSet<_>>();
        for token in unique {
            *document_frequency.entry(token.clone()).or_default() += 1;
        }
    }
    let count = documents.len() as f64;
    let idf = document_frequency
        .into_iter()
        .map(|(token, frequency)| {
            let frequency = frequency as f64;
            (token, (1.0 + (count - frequency + 0.5) / (frequency + 0.5)).ln())
        })
        .collect();
    CorpusStats {
        idf,
        average_document_length: if documents.is_empty() {
            0.0
        } else {
            total_length as f64 / count
        },
    }
}

pub fn bm25_score(query: &[String], document: &[String], stats: &CorpusStats) -> f64 {
    if query.is_empty() || document.is_empty() {
        return 0.0;
    }
    let mut frequencies = HashMap::<&str, usize>::new();
    for token in document {
        *frequencies.entry(token).or_default() += 1;
    }
    let norm = document.len() as f64 / stats.average_document_length.max(1.0);
    query.iter().fold(0.0, |score, token| {
        let Some(frequency) = frequencies.get(token.as_str()) else { return score };
        let idf = stats.idf.get(token).copied().unwrap_or(0.0);
        let frequency = *frequency as f64;
        score + idf * (frequency * 2.5) / (frequency + 1.5 * (1.0 - 0.75 + 0.75 * norm))
    })
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() {
        return 0.0;
    }
    let (dot, left_norm, right_norm) = left.iter().zip(right).fold(
        (0.0_f64, 0.0_f64, 0.0_f64),
        |(dot, left_norm, right_norm), (left, right)| {
            let left = f64::from(*left);
            let right = f64::from(*right);
            (dot + left * right, left_norm + left * left, right_norm + right * right)
        },
    );
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator > 1e-9 { dot / denominator } else { 0.0 }
}

pub fn hybrid_scores(cosine: &[f64], bm25: &[f64], alpha: f64) -> Option<Vec<f64>> {
    if cosine.len() != bm25.len() || !(0.0..=1.0).contains(&alpha) {
        return None;
    }
    let normalise = |scores: &[f64]| {
        let low = scores.iter().copied().fold(f64::INFINITY, f64::min);
        let high = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if (high - low) < 1e-9 { vec![0.5; scores.len()] }
        else { scores.iter().map(|score| (score - low) / (high - low)).collect() }
    };
    let cosine = normalise(cosine);
    let bm25 = normalise(bm25);
    Some(cosine.into_iter().zip(bm25).map(|(dense, lexical)| alpha * dense + (1.0 - alpha) * lexical).collect())
}

#[derive(Debug, Clone)]
pub struct Ranked<T> {
    pub value: T,
    pub embedding: Vec<f32>,
    pub relevance: f64,
}

pub fn mmr_rerank<T: Clone>(mut candidates: Vec<Ranked<T>>, limit: usize, lambda: f64) -> Vec<Ranked<T>> {
    if !(0.0..=1.0).contains(&lambda) { return Vec::new(); }
    let mut chosen = Vec::new();
    while chosen.len() < limit && !candidates.is_empty() {
        let (best, _) = candidates.iter().enumerate().map(|(index, candidate)| {
            let duplicate = chosen.iter().map(|picked: &Ranked<T>| cosine_similarity(&candidate.embedding, &picked.embedding)).fold(0.0, f64::max);
            (index, lambda * candidate.relevance - (1.0 - lambda) * duplicate)
        }).max_by(|(_, left), (_, right)| left.total_cmp(right)).unwrap();
        chosen.push(candidates.remove(best));
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_node_token_and_bm25_behavior() {
        let documents = [tokenize("fix authentication token handling"), tokenize("release release notes")];
        let stats = build_corpus_stats(&documents);
        let query = tokenize("authentication token");
        assert!(bm25_score(&query, &documents[0], &stats) > bm25_score(&query, &documents[1], &stats));
    }

    #[test]
    fn combines_scores_and_suppresses_duplicate_vectors() {
        let scores = hybrid_scores(&[0.9, 0.8, 0.1], &[0.1, 0.9, 0.0], 0.6).unwrap();
        let ranked = mmr_rerank(vec![
            Ranked { value: "first", embedding: vec![1.0, 0.0], relevance: scores[0] },
            Ranked { value: "duplicate", embedding: vec![0.99, 0.01], relevance: scores[1] },
            Ranked { value: "diverse", embedding: vec![0.0, 1.0], relevance: scores[2] },
        ], 2, 0.5);
        assert_eq!(ranked[0].value, "duplicate");
        assert_eq!(ranked[1].value, "diverse");
    }
}
