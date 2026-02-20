//! Search functionality for diagrams and file index.
//!
//! Hybrid scoring combines field-boost (term overlap on structured fields),
//! description-boost (term overlap on NL descriptions), and TF-IDF cosine
//! similarity from the semantic index.

use std::collections::HashMap;
use std::path::Path;
use tracing::info;

use crate::error::Result;
use crate::graphrag;
use crate::indexer::types::FileRecord;
use crate::planner;
use crate::planner::types::ManifestEntry;

/// Weight for semantic (TF-IDF cosine) similarity in the combined score.
const SEMANTIC_WEIGHT: f32 = 5.0;

/// Weight per matching term in the description field.
const DESCRIPTION_TERM_WEIGHT: f32 = 2.5;

/// A search result with relevance score.
#[derive(Debug)]
pub struct SearchResult {
    pub kind: ResultKind,
    pub path: String,
    pub label: String,
    pub score: f32,
    pub snippet: String,
}

/// The kind of entity a search result refers to.
#[derive(Debug)]
pub enum ResultKind {
    File,
    Diagram,
    Domain,
}

/// Search the index for items matching a natural-language query.
///
/// Scoring combines:
/// - Field-boost: term overlap on id, scope, type, source fields
/// - Description-boost: term overlap on NL description
/// - Semantic: TF-IDF cosine similarity (when semantic index available)
pub fn search_index(root: &Path, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let manifest = planner::load_manifest(root)?;
    let files = planner::load_file_index(root)?;

    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    // Load semantic index (graceful degradation if absent)
    let semantic_index = graphrag::index::load_index(root);
    let semantic_scores: HashMap<usize, f32> = semantic_index
        .as_ref()
        .map(|idx| {
            graphrag::index::search_semantic(idx, query, manifest.len())
                .into_iter()
                .map(|r| (r.manifest_idx, r.similarity))
                .collect()
        })
        .unwrap_or_default();

    let mut results = Vec::new();

    // Search diagrams
    for (i, entry) in manifest.iter().enumerate() {
        let field_score = score_manifest_entry(entry, &query_terms);
        let desc_score = score_description(&entry.description, &query_terms);
        let sem_score = semantic_scores.get(&i).copied().unwrap_or(0.0) * SEMANTIC_WEIGHT;

        let total = field_score + desc_score + sem_score;
        if total > 0.0 {
            let snippet = if !entry.description.is_empty() {
                entry.description.clone()
            } else {
                format!("Scope: {}", entry.scope)
            };

            results.push(SearchResult {
                kind: ResultKind::Diagram,
                path: entry.source.clone(),
                label: format!(
                    "{} ({}, ~{} tokens)",
                    entry.id, entry.diagram_type, entry.tokens_est
                ),
                score: total,
                snippet,
            });
        }
    }

    // Search files
    for file in &files {
        let score = score_file(file, &query_terms);
        if score > 0.0 {
            results.push(SearchResult {
                kind: ResultKind::File,
                path: file.path.clone(),
                label: format!("{} ({})", file.path, file.file_type),
                score,
                snippet: format!("Domain: {}, Subdomain: {}", file.domain, file.subdomain),
            });
        }
    }

    // Search domains (aggregate)
    let mut domains: Vec<String> = files.iter().map(|f| f.domain.clone()).collect();
    domains.sort();
    domains.dedup();

    for domain in &domains {
        let domain_lower = domain.to_lowercase();
        if query_terms.iter().any(|t| domain_lower.contains(t)) {
            let file_count = files.iter().filter(|f| &f.domain == domain).count();
            let diag_count = manifest.iter().filter(|m| &m.scope == domain).count();
            results.push(SearchResult {
                kind: ResultKind::Domain,
                path: domain.clone(),
                label: format!("{domain} ({file_count} files, {diag_count} diagrams)"),
                score: 2.0,
                snippet: String::new(),
            });
        }
    }

    // Sort by score descending, then truncate
    results
        .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);

    info!(query, results = results.len(), "Search complete");
    Ok(results)
}

fn score_manifest_entry(entry: &ManifestEntry, terms: &[&str]) -> f32 {
    let mut score = 0.0f32;
    let id_lower = entry.id.to_lowercase();
    let source_lower = entry.source.to_lowercase();
    let scope_lower = entry.scope.to_lowercase();
    let type_lower = entry.diagram_type.to_lowercase();

    for term in terms {
        if id_lower.contains(term) {
            score += 3.0;
        }
        if scope_lower.contains(term) {
            score += 2.0;
        }
        if type_lower.contains(term) {
            score += 1.5;
        }
        if source_lower.contains(term) {
            score += 1.0;
        }
    }

    score
}

/// Score description field: DESCRIPTION_TERM_WEIGHT per matching query term.
fn score_description(description: &str, terms: &[&str]) -> f32 {
    if description.is_empty() {
        return 0.0;
    }

    let desc_lower = description.to_lowercase();
    let mut score = 0.0f32;

    for term in terms {
        if desc_lower.contains(term) {
            score += DESCRIPTION_TERM_WEIGHT;
        }
    }

    score
}

fn score_file(file: &FileRecord, terms: &[&str]) -> f32 {
    let mut score = 0.0f32;
    let path_lower = file.path.to_lowercase();
    let domain_lower = file.domain.to_lowercase();
    let type_lower = file.file_type.to_lowercase();
    let sub_lower = file.subdomain.to_lowercase();

    for term in terms {
        if path_lower.contains(term) {
            score += 2.0;
        }
        if domain_lower.contains(term) {
            score += 1.5;
        }
        if type_lower.contains(term) {
            score += 1.0;
        }
        if sub_lower.contains(term) {
            score += 1.0;
        }
    }

    score
}

/// Format search results for human-readable display.
pub fn format_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }

    let mut out = String::new();
    for (i, r) in results.iter().enumerate() {
        let kind_label = match r.kind {
            ResultKind::File => "[file]",
            ResultKind::Diagram => "[diagram]",
            ResultKind::Domain => "[domain]",
        };
        out.push_str(&format!(
            "{}. {} {} (score: {:.1})\n",
            i + 1,
            kind_label,
            r.label,
            r.score
        ));
        if !r.snippet.is_empty() {
            out.push_str(&format!("   {}\n", r.snippet));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_file_matches_path() {
        let file = FileRecord {
            path: "apps/web/src/auth.ts".to_string(),
            file_type: "module".to_string(),
            domain: "web".to_string(),
            subdomain: "core".to_string(),
            claude_md: String::new(),
        };

        let score = score_file(&file, &["auth"]);
        assert!(score > 0.0);
    }

    #[test]
    fn score_file_no_match() {
        let file = FileRecord {
            path: "apps/web/src/index.ts".to_string(),
            file_type: "module".to_string(),
            domain: "web".to_string(),
            subdomain: "core".to_string(),
            claude_md: String::new(),
        };

        let score = score_file(&file, &["database"]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn score_file_multiple_term_hits() {
        let file = FileRecord {
            path: "apps/web/src/auth.ts".to_string(),
            file_type: "module".to_string(),
            domain: "web".to_string(),
            subdomain: "core".to_string(),
            claude_md: String::new(),
        };

        let multi = score_file(&file, &["web", "auth"]);
        let single = score_file(&file, &["auth"]);
        assert!(multi > single);
    }

    #[test]
    fn score_manifest_entry_matches_id() {
        let entry = ManifestEntry {
            id: "auth-flow".to_string(),
            source: "docs/auth.mmd".to_string(),
            scope: "web".to_string(),
            diagram_type: "flowchart".to_string(),
            tokens_est: 100,
            description: String::new(),
        };

        let score = score_manifest_entry(&entry, &["auth"]);
        assert!(score >= 3.0); // id match (3.0) + source match (1.0) at minimum
    }

    #[test]
    fn score_manifest_entry_no_match() {
        let entry = ManifestEntry {
            id: "root-arch".to_string(),
            source: "docs/arch.mmd".to_string(),
            scope: "root".to_string(),
            diagram_type: "flowchart".to_string(),
            tokens_est: 50,
            description: String::new(),
        };

        let score = score_manifest_entry(&entry, &["database"]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn score_description_matches_terms() {
        let desc = "Sequence diagram showing payment processing between gateway and stripe";
        let score = score_description(desc, &["payment", "gateway"]);
        assert!((score - 2.0 * DESCRIPTION_TERM_WEIGHT).abs() < 0.001);
    }

    #[test]
    fn score_description_empty() {
        let score = score_description("", &["payment"]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn score_description_no_match() {
        let desc = "Flowchart showing authentication flow";
        let score = score_description(desc, &["payment", "database"]);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn format_results_empty() {
        assert_eq!(format_results(&[]), "No results found.");
    }

    #[test]
    fn format_results_single() {
        let results = vec![SearchResult {
            kind: ResultKind::File,
            path: "src/main.rs".to_string(),
            label: "src/main.rs (module)".to_string(),
            score: 2.0,
            snippet: "Domain: root".to_string(),
        }];

        let out = format_results(&results);
        assert!(out.contains("[file]"));
        assert!(out.contains("src/main.rs"));
        assert!(out.contains("score: 2.0"));
        assert!(out.contains("Domain: root"));
    }

    #[test]
    fn search_index_requires_index() {
        let dir = tempfile::tempdir().unwrap();
        let result = search_index(dir.path(), "auth", 10);
        assert!(result.is_err());
    }

    #[test]
    fn search_index_with_data() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let manifest = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "diagrams": [
                {
                    "id": "auth-flow",
                    "source": "docs/auth.mmd",
                    "domain": "web",
                    "type": "flowchart",
                    "tokens_est": 80
                }
            ]
        });
        std::fs::write(
            claude_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let index = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "root": "test",
            "claude_md_paths": [],
            "files": [
                {
                    "path": "apps/web/src/auth.ts",
                    "file_type": "module",
                    "domain": "web",
                    "subdomain": "core",
                    "claude_md": ""
                },
                {
                    "path": "apps/web/src/index.ts",
                    "file_type": "module",
                    "domain": "web",
                    "subdomain": "core",
                    "claude_md": ""
                }
            ]
        });
        std::fs::write(
            claude_dir.join("file-index.json"),
            serde_json::to_string_pretty(&index).unwrap(),
        )
        .unwrap();

        let results = search_index(dir.path(), "auth", 10).unwrap();
        assert!(!results.is_empty());
        // "auth" should match the diagram id and the file path
        assert!(results.len() >= 2);
    }

    #[test]
    fn search_uses_description_for_scoring() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let manifest = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "diagrams": [
                {
                    "id": "payment-seq",
                    "source": "docs/payment.mmd",
                    "domain": "web",
                    "type": "sequence",
                    "tokens_est": 100,
                    "description": "Sequence diagram showing payment processing between user gateway and stripe"
                },
                {
                    "id": "root-arch",
                    "source": "docs/arch.mmd",
                    "domain": "root",
                    "type": "flowchart",
                    "tokens_est": 50
                }
            ]
        });
        std::fs::write(
            claude_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let file_index = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "root": "test",
            "claude_md_paths": [],
            "files": []
        });
        std::fs::write(
            claude_dir.join("file-index.json"),
            serde_json::to_string_pretty(&file_index).unwrap(),
        )
        .unwrap();

        let results = search_index(dir.path(), "payment gateway stripe", 10).unwrap();
        assert!(!results.is_empty());
        // Payment diagram should be found via description match even though
        // "payment" isn't in the id/scope/type fields significantly
        let payment_result = results.iter().find(|r| r.path == "docs/payment.mmd");
        assert!(payment_result.is_some());
        assert!(payment_result.unwrap().score > 0.0);
        // Description should be used as snippet
        assert!(payment_result.unwrap().snippet.contains("payment"));
    }

    #[test]
    fn search_gracefully_degrades_without_semantic_index() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let manifest = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "diagrams": [
                {
                    "id": "auth-flow",
                    "source": "docs/auth.mmd",
                    "domain": "web",
                    "type": "flowchart",
                    "tokens_est": 80
                }
            ]
        });
        std::fs::write(
            claude_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let file_index = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "root": "test",
            "claude_md_paths": [],
            "files": []
        });
        std::fs::write(
            claude_dir.join("file-index.json"),
            serde_json::to_string_pretty(&file_index).unwrap(),
        )
        .unwrap();

        // No semantic-index.json present — should still work with field-boost only
        let results = search_index(dir.path(), "auth", 10).unwrap();
        assert!(!results.is_empty());
    }
}
