#[derive(Debug, Clone, PartialEq)]
pub struct CompressionResult {
    pub compressed: String,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub preserved_spans: usize,
    pub sentences_kept: usize,
    pub sentences_total: usize,
}

pub fn compress_message(
    message: &str,
    budget_tokens: usize,
    mode: &str,
) -> Result<CompressionResult, String> {
    if message.is_empty() {
        return Err("No message provided. Use --message or --message-file.".into());
    }
    if budget_tokens == 0 || !matches!(mode, "keyword" | "sentence" | "hybrid") {
        return Err("budget must be positive and mode must be keyword, sentence, or hybrid".into());
    }
    let sentences = split_sentences(message);
    let mut kept = Vec::<(usize, &str)>::new();
    let mut used = 0;
    for (index, sentence) in sentences.iter().enumerate() {
        if is_preserved(sentence) {
            used += estimate_tokens(sentence);
            kept.push((index, sentence));
        }
    }
    let mut ranked = sentences
        .iter()
        .enumerate()
        .filter(|(_, sentence)| !is_preserved(sentence))
        .map(|(index, sentence)| (index, *sentence, score(sentence, index, mode)))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.2.total_cmp(&left.2));
    for (index, sentence, _) in ranked {
        let cost = estimate_tokens(sentence);
        if used + cost <= budget_tokens {
            used += cost;
            kept.push((index, sentence));
        }
    }
    kept.sort_by_key(|(index, _)| *index);
    let compressed = kept
        .iter()
        .map(|(_, sentence)| *sentence)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(CompressionResult {
        compressed_tokens: estimate_tokens(&compressed),
        original_tokens: estimate_tokens(message),
        preserved_spans: sentences
            .iter()
            .filter(|sentence| is_preserved(sentence))
            .count(),
        sentences_kept: kept.len(),
        sentences_total: sentences.len(),
        compressed,
    })
}

fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

fn split_sentences(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if matches!(character, '.' | '!' | '?')
            && text[index + character.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            let sentence = text[start..index + character.len_utf8()].trim();
            if !sentence.is_empty() {
                result.push(sentence);
            }
            start = index + character.len_utf8();
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        result.push(tail);
    }
    result
}

fn is_preserved(sentence: &str) -> bool {
    sentence.contains("```")
        || sentence.contains('`')
        || sentence.contains("http://")
        || sentence.contains("https://")
        || sentence
            .split_whitespace()
            .any(|word| word.contains('/') && word.contains('.'))
}

fn score(sentence: &str, index: usize, mode: &str) -> f64 {
    let keyword = sentence
        .split_whitespace()
        .filter(|word| {
            word.len() > 2
                && !matches!(
                    *word,
                    "the" | "and" | "with" | "from" | "that" | "this" | "for" | "are" | "was"
                )
        })
        .count() as f64;
    let sentence_score =
        (sentence.split_whitespace().count() as f64 + 1.0).ln() - index as f64 * 0.05;
    match mode {
        "keyword" => keyword,
        "sentence" => sentence_score,
        _ => 0.7 * keyword + 0.3 * sentence_score,
    }
}

#[cfg(test)]
mod tests {
    use super::compress_message;
    #[test]
    fn preserves_load_bearing_spans_and_obeys_the_budget_when_possible() {
        let result = compress_message("Ignore filler text. Keep https://example.test/api. Important security remediation follows.", 20, "hybrid").unwrap();
        assert!(result.compressed.contains("https://example.test/api"));
        assert!(result.compressed_tokens <= 20);
    }
}
