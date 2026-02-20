//! Semantic index: build, persist, load, and search TF-IDF vectors for diagrams.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::output_dir;
use crate::error::{self, Error, Result};
use crate::planner::types::ManifestEntry;

use super::tfidf;

/// A document vector associated with a diagram ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocVector {
    pub diagram_id: String,
    pub vector: HashMap<String, f32>,
}

/// The complete semantic index: IDF table + per-document TF-IDF vectors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticIndex {
    pub idf: HashMap<String, f32>,
    pub doc_vectors: Vec<DocVector>,
    pub doc_count: usize,
}

/// A semantic search result: index into the manifest + similarity score.
#[derive(Debug)]
pub struct SemanticResult {
    pub manifest_idx: usize,
    pub similarity: f32,
}

const SEMANTIC_INDEX_FILE: &str = "semantic-index.json";

/// Build a semantic index from manifest entries.
///
/// For each diagram, concatenates the description, ID, scope, and type
/// into a document, tokenizes, computes TF-IDF vectors.
pub fn build_index(entries: &[ManifestEntry]) -> SemanticIndex {
    if entries.is_empty() {
        return SemanticIndex {
            idf: HashMap::new(),
            doc_vectors: Vec::new(),
            doc_count: 0,
        };
    }

    // Tokenize all documents
    let doc_tokens: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            let text = format!(
                "{} {} {} {}",
                e.description, e.id, e.scope, e.diagram_type
            );
            tfidf::tokenize(&text)
        })
        .collect();

    // Compute IDF across all documents
    let idf = tfidf::inverse_document_frequencies(&doc_tokens);

    // Compute TF-IDF vectors for each document
    let doc_vectors: Vec<DocVector> = entries
        .iter()
        .zip(doc_tokens.iter())
        .map(|(entry, tokens)| {
            let tf = tfidf::term_frequencies(tokens);
            let vector = tfidf::tfidf_vector(&tf, &idf);
            DocVector {
                diagram_id: entry.id.clone(),
                vector,
            }
        })
        .collect();

    let doc_count = entries.len();
    info!(doc_count, terms = idf.len(), "Built semantic index");

    SemanticIndex {
        idf,
        doc_vectors,
        doc_count,
    }
}

/// Persist the semantic index to `.claude/semantic-index.json`.
pub fn write_index(root: &Path, index: &SemanticIndex) -> Result<()> {
    let out = output_dir(root);
    std::fs::create_dir_all(&out).map_err(|e| error::io_err(&out, e))?;

    let path = out.join(SEMANTIC_INDEX_FILE);
    let json = serde_json::to_string_pretty(index)
        .map_err(|e| Error::Json { path: path.clone(), source: e })?;
    std::fs::write(&path, &json).map_err(|e| error::io_err(&path, e))?;

    info!(
        path = %path.display(),
        docs = index.doc_count,
        terms = index.idf.len(),
        "Wrote semantic index"
    );
    Ok(())
}

/// Load the semantic index from `.claude/semantic-index.json`.
///
/// Returns `None` if the file doesn't exist (graceful degradation).
pub fn load_index(root: &Path) -> Option<SemanticIndex> {
    let path = output_dir(root).join(SEMANTIC_INDEX_FILE);
    if !path.exists() {
        return None;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not read semantic index");
            return None;
        }
    };

    match serde_json::from_str(&content) {
        Ok(idx) => Some(idx),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not parse semantic index");
            None
        }
    }
}

/// Search the semantic index for diagrams similar to a query.
///
/// Returns results sorted by descending similarity, limited to `limit`.
pub fn search_semantic(
    index: &SemanticIndex,
    query: &str,
    limit: usize,
) -> Vec<SemanticResult> {
    let query_tokens = tfidf::tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let query_tf = tfidf::term_frequencies(&query_tokens);
    let query_vec = tfidf::tfidf_vector(&query_tf, &index.idf);

    let mut results: Vec<SemanticResult> = index
        .doc_vectors
        .iter()
        .enumerate()
        .map(|(idx, doc)| SemanticResult {
            manifest_idx: idx,
            similarity: tfidf::cosine_similarity(&query_vec, &doc.vector),
        })
        .filter(|r| r.similarity > 0.0)
        .collect();

    results.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries() -> Vec<ManifestEntry> {
        vec![
            ManifestEntry {
                id: "payment-flow".to_string(),
                source: "docs/payment.mmd".to_string(),
                scope: "web".to_string(),
                diagram_type: "sequence".to_string(),
                tokens_est: 100,
                description: "Sequence diagram showing payment processing between user, gateway, and stripe.".to_string(),
            },
            ManifestEntry {
                id: "auth-flow".to_string(),
                source: "docs/auth.mmd".to_string(),
                scope: "web".to_string(),
                diagram_type: "flowchart".to_string(),
                tokens_est: 80,
                description: "Flowchart showing authentication login flow with OAuth and JWT tokens.".to_string(),
            },
            ManifestEntry {
                id: "db-schema".to_string(),
                source: "docs/schema.mmd".to_string(),
                scope: "supabase".to_string(),
                diagram_type: "erDiagram".to_string(),
                tokens_est: 120,
                description: "ER diagram defining users, orders, products. Relationships: places, contains.".to_string(),
            },
        ]
    }

    #[test]
    fn build_index_creates_vectors() {
        let entries = make_entries();
        let index = build_index(&entries);
        assert_eq!(index.doc_count, 3);
        assert_eq!(index.doc_vectors.len(), 3);
        assert!(!index.idf.is_empty());
    }

    #[test]
    fn build_index_empty() {
        let index = build_index(&[]);
        assert_eq!(index.doc_count, 0);
        assert!(index.doc_vectors.is_empty());
        assert!(index.idf.is_empty());
    }

    #[test]
    fn search_semantic_finds_payment() {
        let entries = make_entries();
        let index = build_index(&entries);
        let results = search_semantic(&index, "how is payment processed", 10);
        assert!(!results.is_empty());
        // Payment diagram should rank first
        assert_eq!(results[0].manifest_idx, 0);
        assert!(results[0].similarity > 0.0);
    }

    #[test]
    fn search_semantic_finds_auth() {
        let entries = make_entries();
        let index = build_index(&entries);
        let results = search_semantic(&index, "authentication login OAuth", 10);
        assert!(!results.is_empty());
        // Auth diagram should rank first
        assert_eq!(results[0].manifest_idx, 1);
    }

    #[test]
    fn search_semantic_finds_database() {
        let entries = make_entries();
        let index = build_index(&entries);
        let results = search_semantic(&index, "users orders products database", 10);
        assert!(!results.is_empty());
        // DB schema should rank first
        assert_eq!(results[0].manifest_idx, 2);
    }

    #[test]
    fn search_semantic_empty_query() {
        let entries = make_entries();
        let index = build_index(&entries);
        let results = search_semantic(&index, "", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn search_semantic_no_overlap() {
        let entries = make_entries();
        let index = build_index(&entries);
        let results = search_semantic(&index, "kubernetes terraform", 10);
        // No overlap with any diagram descriptions
        assert!(results.is_empty());
    }

    #[test]
    fn write_and_load_index() {
        let dir = tempfile::tempdir().unwrap();
        let entries = make_entries();
        let index = build_index(&entries);

        write_index(dir.path(), &index).unwrap();

        let loaded = load_index(dir.path());
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.doc_count, index.doc_count);
        assert_eq!(loaded.doc_vectors.len(), index.doc_vectors.len());
    }

    #[test]
    fn load_index_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_index(dir.path());
        assert!(loaded.is_none());
    }

    #[test]
    fn search_respects_limit() {
        let entries = make_entries();
        let index = build_index(&entries);
        let results = search_semantic(&index, "web diagram flow", 1);
        assert!(results.len() <= 1);
    }
}
