//! Data structures for the indexer.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single file in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    /// Relative POSIX path from repo root.
    pub path: String,
    /// Semantic type (component, module, migration, etc.).
    pub file_type: String,
    /// Logical domain (web, admin, supabase, etc.).
    pub domain: String,
    /// Semantic subdomain / architectural layer.
    pub subdomain: String,
    /// Path to nearest CLAUDE.md, or empty.
    pub claude_md: String,
}

/// A mermaid diagram found in the repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramRecord {
    /// Stable ID (from HTML comment or auto-generated).
    pub id: String,
    /// Source file path (+ `#diag-N` suffix if multiple in one file).
    pub source: String,
    /// Domain scope.
    pub domain: String,
    /// Inferred mermaid diagram type (flowchart, erDiagram, etc.).
    pub diagram_type: String,
    /// Estimated token count.
    pub tokens_est: usize,
    /// The minified mermaid source content.
    pub content: String,
    /// Natural language description for semantic search.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Complete result of an indexing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexResult {
    /// Repo root (for display, not serialized as absolute).
    #[serde(skip)]
    pub root: PathBuf,
    /// ISO 8601 timestamp of generation.
    pub generated_at: String,
    /// Repo name (last component of root).
    pub root_name: String,
    /// All indexed files.
    pub files: Vec<FileRecord>,
    /// Extracted mermaid diagrams.
    pub diagrams: Vec<DiagramRecord>,
    /// Paths to all CLAUDE.md files found.
    pub claude_md_paths: Vec<String>,
}
