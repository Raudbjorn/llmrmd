//! TF-IDF vectorization: tokenization, term frequencies, IDF, cosine similarity.
//!
//! Pure Rust implementation using only `std::collections`. No external dependencies.

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;

/// English stop words + Mermaid-specific noise words.
static STOP_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        // English basics
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "shall",
        "should", "may", "might", "must", "can", "could", "to", "of", "in",
        "for", "on", "with", "at", "by", "from", "as", "into", "through",
        "during", "before", "after", "above", "below", "between", "out",
        "off", "over", "under", "again", "further", "then", "once", "here",
        "there", "when", "where", "why", "how", "all", "each", "every",
        "both", "few", "more", "most", "other", "some", "such", "no", "nor",
        "not", "only", "own", "same", "so", "than", "too", "very", "just",
        "because", "but", "and", "or", "if", "while", "about", "up", "its",
        "it", "this", "that", "these", "those", "he", "she", "they", "we",
        "you", "me", "him", "her", "us", "them", "my", "your", "his",
        "their", "our", "what", "which", "who", "whom",
        // Mermaid noise
        "diagram", "flowchart", "graph", "subgraph", "end", "style",
        "classDef", "class", "click", "link", "direction", "participant",
        "note", "loop", "alt", "opt", "par", "rect", "activate", "deactivate",
        "title", "section",
    ]
    .into_iter()
    .collect()
});

/// Tokenize text: lowercase, split on non-alphanumeric, filter stop words and short tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 2)
        .filter(|w| !STOP_WORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Compute sublinear term frequencies: `1.0 + ln(count)` for each term.
pub fn term_frequencies(tokens: &[String]) -> HashMap<String, f32> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for token in tokens {
        *counts.entry(token.clone()).or_default() += 1;
    }

    counts
        .into_iter()
        .map(|(term, count)| {
            let tf = 1.0 + (count as f32).ln();
            (term, tf)
        })
        .collect()
}

/// Compute inverse document frequencies: `ln(N / (1 + df))` for each term across documents.
pub fn inverse_document_frequencies(docs: &[Vec<String>]) -> HashMap<String, f32> {
    let n = docs.len() as f32;
    let mut df: HashMap<String, usize> = HashMap::new();

    for doc in docs {
        let unique: HashSet<&String> = doc.iter().collect();
        for term in unique {
            *df.entry(term.clone()).or_default() += 1;
        }
    }

    df.into_iter()
        .map(|(term, count)| {
            let idf = (n / (1.0 + count as f32)).ln();
            (term, idf)
        })
        .collect()
}

/// Compute a sparse TF-IDF vector from term frequencies and IDF values.
pub fn tfidf_vector(tf: &HashMap<String, f32>, idf: &HashMap<String, f32>) -> HashMap<String, f32> {
    tf.iter()
        .filter_map(|(term, &tf_val)| {
            idf.get(term).map(|&idf_val| (term.clone(), tf_val * idf_val))
        })
        .collect()
}

/// Compute cosine similarity between two sparse vectors.
pub fn cosine_similarity(a: &HashMap<String, f32>, b: &HashMap<String, f32>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let dot: f32 = a
        .iter()
        .filter_map(|(term, &a_val)| b.get(term).map(|&b_val| a_val * b_val))
        .sum();

    let mag_a: f32 = a.values().map(|v| v * v).sum::<f32>().sqrt();
    let mag_b: f32 = b.values().map(|v| v * v).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_basic() {
        let tokens = tokenize("Hello World, this is a test!");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Stop words filtered
        assert!(!tokens.contains(&"this".to_string()));
        assert!(!tokens.contains(&"is".to_string()));
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn tokenize_filters_short_tokens() {
        let tokens = tokenize("I am x y ok fine");
        // "I", "x", "y" are too short (< 2 chars)
        assert!(!tokens.contains(&"i".to_string()));
        assert!(!tokens.contains(&"x".to_string()));
        assert!(!tokens.contains(&"y".to_string()));
        assert!(tokens.contains(&"ok".to_string()));
        assert!(tokens.contains(&"fine".to_string()));
    }

    #[test]
    fn tokenize_filters_mermaid_noise() {
        let tokens = tokenize("flowchart diagram participant subgraph payment");
        assert!(!tokens.contains(&"flowchart".to_string()));
        assert!(!tokens.contains(&"diagram".to_string()));
        assert!(!tokens.contains(&"participant".to_string()));
        assert!(tokens.contains(&"payment".to_string()));
    }

    #[test]
    fn tokenize_splits_on_special_chars() {
        let tokens = tokenize("user-auth/login.handler");
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"login".to_string()));
        assert!(tokens.contains(&"handler".to_string()));
    }

    #[test]
    fn term_frequencies_sublinear() {
        let tokens = vec!["auth".to_string(), "auth".to_string(), "login".to_string()];
        let tf = term_frequencies(&tokens);
        // "auth" appears 2 times: 1.0 + ln(2) ≈ 1.693
        assert!((tf["auth"] - (1.0 + 2.0f32.ln())).abs() < 0.001);
        // "login" appears 1 time: 1.0 + ln(1) = 1.0
        assert!((tf["login"] - 1.0).abs() < 0.001);
    }

    #[test]
    fn idf_basic() {
        let docs = vec![
            vec!["auth".to_string(), "login".to_string()],
            vec!["auth".to_string(), "payment".to_string()],
            vec!["payment".to_string(), "checkout".to_string()],
        ];
        let idf = inverse_document_frequencies(&docs);
        // "auth" appears in 2 docs: ln(3 / (1+2)) = ln(1) = 0
        assert!((idf["auth"] - 0.0).abs() < 0.001);
        // "checkout" appears in 1 doc: ln(3 / (1+1)) = ln(1.5) ≈ 0.405
        assert!((idf["checkout"] - (3.0f32 / 2.0).ln()).abs() < 0.001);
    }

    #[test]
    fn tfidf_vector_basic() {
        let mut tf = HashMap::new();
        tf.insert("auth".to_string(), 1.5);
        tf.insert("login".to_string(), 1.0);

        let mut idf = HashMap::new();
        idf.insert("auth".to_string(), 0.5);
        idf.insert("login".to_string(), 1.2);
        idf.insert("other".to_string(), 0.8);

        let vec = tfidf_vector(&tf, &idf);
        assert!((vec["auth"] - 0.75).abs() < 0.001);
        assert!((vec["login"] - 1.2).abs() < 0.001);
        assert!(!vec.contains_key("other")); // not in tf
    }

    #[test]
    fn cosine_similarity_identical() {
        let mut a = HashMap::new();
        a.insert("x".to_string(), 1.0);
        a.insert("y".to_string(), 2.0);
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let mut a = HashMap::new();
        a.insert("x".to_string(), 1.0);
        let mut b = HashMap::new();
        b.insert("y".to_string(), 1.0);
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_empty() {
        let a: HashMap<String, f32> = HashMap::new();
        let b: HashMap<String, f32> = HashMap::new();
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_similarity_partial_overlap() {
        let mut a = HashMap::new();
        a.insert("auth".to_string(), 1.0);
        a.insert("login".to_string(), 1.0);

        let mut b = HashMap::new();
        b.insert("auth".to_string(), 1.0);
        b.insert("payment".to_string(), 1.0);

        let sim = cosine_similarity(&a, &b);
        // dot = 1*1 = 1, mag_a = sqrt(2), mag_b = sqrt(2), sim = 1/2 = 0.5
        assert!((sim - 0.5).abs() < 0.001);
    }
}
