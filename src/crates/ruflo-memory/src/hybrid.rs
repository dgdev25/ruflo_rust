//! Source-compatible hybrid ranking primitives.
//!
//! Ported from `v3/@claude-flow/cli/src/memory/hybrid-retrieval.ts`: lexical
//! BM25 supplements dense cosine, then MMR removes near-duplicate results.

use std::collections::{HashMap, HashSet};

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "if", "then", "else", "of", "in", "to", "for", "on",
    "at", "by", "with", "from", "is", "are", "was", "were", "be", "been", "being", "have", "has",
    "had", "do", "does", "did", "will", "would", "should", "could", "can", "may", "might", "must",
    "this", "that", "these", "those", "it", "its", "as", "also", "not", "no", "so", "too", "very",
];

#[derive(Debug, Clone)]
pub struct CorpusStats {
    idf: HashMap<String, f64>,
    average_document_length: f64,
}

/// Node's default classifier for release/merge metadata that otherwise tends
/// to dominate small-corpus retrieval results.
pub fn type_penalty(name: Option<&str>, factor: f64) -> f64 {
    if name.is_none() || !factor.is_finite() || factor < 0.0 {
        return 1.0;
    }
    let name = name.unwrap().trim_start().to_ascii_lowercase();
    if name.starts_with("chore(release)")
        || name.starts_with("merge ")
        || name.starts_with("bump ")
        || name.starts_with("publish ")
            && name["publish ".len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        || name.starts_with("[dream cycle")
    {
        factor
    } else {
        1.0
    }
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
            (
                token,
                (1.0 + (count - frequency + 0.5) / (frequency + 0.5)).ln(),
            )
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
        let Some(frequency) = frequencies.get(token.as_str()) else {
            return score;
        };
        let idf = stats.idf.get(token).copied().unwrap_or(0.0);
        let frequency = *frequency as f64;
        score + idf * (frequency * 2.5) / (frequency + 1.5 * (1.0 - 0.75 + 0.75 * norm))
    })
}

/// Node's multi-field lexical policy. Memory keys are the high-signal subject
/// field and memory content is the body field, so callers build independent
/// corpus statistics for each.
pub fn multi_field_bm25(
    query: &[String],
    subject: &[String],
    body: &[String],
    subject_stats: &CorpusStats,
    body_stats: &CorpusStats,
    subject_weight: f64,
    body_weight: f64,
) -> f64 {
    subject_weight * bm25_score(query, subject, subject_stats)
        + body_weight * bm25_score(query, body, body_stats)
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
            (
                dot + left * right,
                left_norm + left * left,
                right_norm + right * right,
            )
        },
    );
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator > 1e-9 {
        dot / denominator
    } else {
        0.0
    }
}

/// Node's exported min/max normalizer. A constant non-empty input becomes
/// `0.5`; an empty input remains empty.
pub fn normalise(scores: &[f64]) -> Vec<f64> {
    if scores.is_empty() {
        return Vec::new();
    }
    let low = scores.iter().copied().fold(f64::INFINITY, f64::min);
    let high = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if (high - low) < 1e-9 {
        vec![0.5; scores.len()]
    } else {
        scores
            .iter()
            .map(|score| (score - low) / (high - low))
            .collect()
    }
}

pub fn hybrid_scores(cosine: &[f64], bm25: &[f64], alpha: f64) -> Option<Vec<f64>> {
    if cosine.len() != bm25.len() || !(0.0..=1.0).contains(&alpha) {
        return None;
    }
    let cosine = normalise(cosine);
    let bm25 = normalise(bm25);
    Some(
        cosine
            .into_iter()
            .zip(bm25)
            .map(|(dense, lexical)| alpha * dense + (1.0 - alpha) * lexical)
            .collect(),
    )
}

#[derive(Debug, Clone)]
pub struct Ranked<T> {
    pub value: T,
    pub embedding: Vec<f32>,
    pub relevance: f64,
}

pub fn mmr_rerank<T: Clone>(
    mut candidates: Vec<Ranked<T>>,
    limit: usize,
    lambda: f64,
) -> Vec<Ranked<T>> {
    if !(0.0..=1.0).contains(&lambda) {
        return Vec::new();
    }
    let mut chosen = Vec::new();
    while chosen.len() < limit && !candidates.is_empty() {
        let mut best = 0;
        let mut best_score = f64::NEG_INFINITY;
        for (index, candidate) in candidates.iter().enumerate() {
            let duplicate = chosen
                .iter()
                .map(|picked: &Ranked<T>| {
                    cosine_similarity(&candidate.embedding, &picked.embedding)
                })
                .fold(0.0, f64::max);
            let score = lambda * candidate.relevance - (1.0 - lambda) * duplicate;
            // Node uses `if (score > bestScore)`, preserving input order on a tie.
            if score > best_score {
                best = index;
                best_score = score;
            }
        }
        chosen.push(candidates.remove(best));
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_node_token_and_bm25_behavior() {
        let documents = [
            tokenize("fix authentication token handling"),
            tokenize("release release notes"),
        ];
        let stats = build_corpus_stats(&documents);
        let query = tokenize("authentication token");
        assert!(
            bm25_score(&query, &documents[0], &stats) > bm25_score(&query, &documents[1], &stats)
        );
    }

    #[test]
    fn combines_scores_and_suppresses_duplicate_vectors() {
        let scores = hybrid_scores(&[0.9, 0.8, 0.1], &[0.1, 0.9, 0.0], 0.6).unwrap();
        let ranked = mmr_rerank(
            vec![
                Ranked {
                    value: "first",
                    embedding: vec![1.0, 0.0],
                    relevance: scores[0],
                },
                Ranked {
                    value: "duplicate",
                    embedding: vec![0.99, 0.01],
                    relevance: scores[1],
                },
                Ranked {
                    value: "diverse",
                    embedding: vec![0.0, 1.0],
                    relevance: scores[2],
                },
            ],
            2,
            0.5,
        );
        assert_eq!(ranked[0].value, "duplicate");
        assert_eq!(ranked[1].value, "diverse");
    }

    #[test]
    fn ports_node_multifield_and_meta_commit_policy() {
        let query = tokenize("auth token");
        let subjects = [tokenize("auth token"), tokenize("chore(release)")];
        let bodies = [tokenize("rotate credentials"), tokenize("auth token notes")];
        let subject_stats = build_corpus_stats(&subjects);
        let body_stats = build_corpus_stats(&bodies);

        let focused = multi_field_bm25(
            &query,
            &subjects[0],
            &bodies[0],
            &subject_stats,
            &body_stats,
            3.0,
            1.0,
        );
        let release = multi_field_bm25(
            &query,
            &subjects[1],
            &bodies[1],
            &subject_stats,
            &body_stats,
            3.0,
            1.0,
        ) * type_penalty(Some("chore(release): publish 3.34.0"), 0.5);
        assert!(focused > release);
        assert_eq!(type_penalty(Some("Merge main"), 0.5), 0.5);
        assert_eq!(type_penalty(Some("feat(memory): rotate token"), 0.5), 1.0);
    }

    #[test]
    fn matches_node_hybrid_source_vectors_and_tie_order() {
        assert_eq!(
            tokenize("Refactor src/Auth/middleware.ts to use jwt-verify!"),
            vec!["refactor", "src/auth/middleware.ts", "use", "jwt-verify"]
        );
        assert_eq!(tokenize("the cat is on a mat"), vec!["cat", "mat"]);
        assert_eq!(normalise(&[0.5, 0.5, 0.5]), vec![0.5, 0.5, 0.5]);
        assert!(
            hybrid_scores(&[0.1, 0.5, 0.9], &[3.0, 1.0, 0.0], 0.5).unwrap()[0]
                > hybrid_scores(&[0.1, 0.5, 0.9], &[3.0, 1.0, 0.0], 0.5).unwrap()[1]
        );

        let ties = mmr_rerank(
            vec![
                Ranked {
                    value: "first",
                    embedding: vec![1.0, 0.0],
                    relevance: 1.0,
                },
                Ranked {
                    value: "second",
                    embedding: vec![1.0, 0.0],
                    relevance: 1.0,
                },
            ],
            1,
            1.0,
        );
        assert_eq!(ties[0].value, "first");
    }
}
