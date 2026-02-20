//! Data structures for the planner.

use serde::{Deserialize, Serialize};

/// A diagram entry from the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub source: String,
    pub scope: String,
    pub diagram_type: String,
    pub tokens_est: usize,
    /// Natural language description for semantic search.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// All the context assembled for a planning session.
#[derive(Debug, Clone)]
pub struct PlannerContext {
    /// The user's change description.
    pub change_description: String,
    /// Inferred (or overridden) relevant scopes.
    pub relevant_scopes: Vec<String>,
    /// Root-level system diagram (always included if exists).
    pub system_diagram: String,
    /// Selected diagrams: (manifest entry, content).
    pub selected_diagrams: Vec<(ManifestEntry, String)>,
    /// Relevant CLAUDE.md files: (path, content).
    pub relevant_claude_mds: Vec<(String, String)>,
    /// Filtered TOON excerpt for relevant domains.
    pub file_index_excerpt: String,
    /// Estimated total tokens used.
    pub total_tokens_est: usize,
}
