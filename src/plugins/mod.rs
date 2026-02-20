//! Language-specific plugin system for AST-to-Mermaid diagram generation.
//!
//! Each language plugin can parse source files and produce Mermaid diagrams
//! at different abstraction levels. The system follows a C4-inspired hierarchy:
//!
//! - **System**: high-level service/package relationships
//! - **Container**: mid-level module interactions
//! - **Class**: detailed module internals (structs, functions, traits)
//!
//! Plugins are registered in [`registry::PluginRegistry`] and discovered
//! automatically based on file extensions.

pub mod builtin;
pub mod registry;
pub mod tiers;

use std::path::Path;

use crate::error::Result;

/// Abstraction level for generated diagrams (C4-inspired hierarchy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DiagramLevel {
    /// System-context: high-level service/package relationships.
    System,
    /// Container/service: mid-level module interactions.
    Container,
    /// Class/function: detailed module internals.
    Class,
}

impl DiagramLevel {
    /// String representation used in file names and labels.
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagramLevel::System => "system",
            DiagramLevel::Container => "container",
            DiagramLevel::Class => "class",
        }
    }

    /// Generate a tier-specific filename for a given domain.
    ///
    /// Example: `DiagramLevel::System.tier_filename("web")` => `"web-system.mmd"`
    pub fn tier_filename(&self, domain: &str) -> String {
        match self {
            DiagramLevel::System => format!("{domain}-system.mmd"),
            DiagramLevel::Container => format!("{domain}-containers.mmd"),
            DiagramLevel::Class => format!("{domain}-classes.mmd"),
        }
    }
}

impl std::fmt::Display for DiagramLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A diagram produced by a language plugin.
#[derive(Debug, Clone)]
pub struct GeneratedDiagram {
    /// Mermaid source content.
    pub content: String,
    /// Abstraction level.
    pub level: DiagramLevel,
    /// Diagram type (flowchart, classDiagram, sequenceDiagram, etc.).
    pub diagram_type: String,
    /// Descriptive label for manifest/index display.
    pub label: String,
}

/// Trait for language-specific diagram generators.
///
/// Implementors parse source files for a given language and produce
/// Mermaid diagrams at one or more abstraction levels. External tool
/// dependencies are checked via [`check_prerequisites`](LanguagePlugin::check_prerequisites)
/// before any generation is attempted.
pub trait LanguagePlugin: Send + Sync {
    /// Human-readable name of this plugin (e.g., "rust", "python").
    fn name(&self) -> &str;

    /// File extensions this plugin handles, with leading dot (e.g., `[".py", ".pyi"]`).
    fn extensions(&self) -> &[&str];

    /// Check whether required external tools are available.
    ///
    /// Returns `Ok(true)` if all prerequisites are met, `Ok(false)` if a tool
    /// is simply missing, or `Err` if the check itself failed unexpectedly.
    fn check_prerequisites(&self) -> Result<bool>;

    /// Generate diagrams from source files.
    ///
    /// `root` is the absolute repo root. `files` contains paths relative to
    /// `root` that match this plugin's extensions.
    fn generate(&self, root: &Path, files: &[&Path]) -> Result<Vec<GeneratedDiagram>>;

    /// The abstraction levels this plugin supports.
    fn supported_levels(&self) -> Vec<DiagramLevel> {
        vec![DiagramLevel::Class]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_level_as_str() {
        assert_eq!(DiagramLevel::System.as_str(), "system");
        assert_eq!(DiagramLevel::Container.as_str(), "container");
        assert_eq!(DiagramLevel::Class.as_str(), "class");
    }

    #[test]
    fn diagram_level_display() {
        assert_eq!(format!("{}", DiagramLevel::System), "system");
        assert_eq!(format!("{}", DiagramLevel::Container), "container");
        assert_eq!(format!("{}", DiagramLevel::Class), "class");
    }

    #[test]
    fn tier_filename_system() {
        assert_eq!(DiagramLevel::System.tier_filename("web"), "web-system.mmd");
    }

    #[test]
    fn tier_filename_container() {
        assert_eq!(
            DiagramLevel::Container.tier_filename("api"),
            "api-containers.mmd"
        );
    }

    #[test]
    fn tier_filename_class() {
        assert_eq!(
            DiagramLevel::Class.tier_filename("shared"),
            "shared-classes.mmd"
        );
    }

    #[test]
    fn diagram_level_eq_and_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DiagramLevel::System);
        set.insert(DiagramLevel::Container);
        set.insert(DiagramLevel::Class);
        assert_eq!(set.len(), 3);

        // Duplicate insert should not increase size.
        set.insert(DiagramLevel::System);
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn diagram_level_clone() {
        let level = DiagramLevel::Container;
        let cloned = level;
        assert_eq!(level, cloned);
    }

    #[test]
    fn generated_diagram_clone() {
        let diag = GeneratedDiagram {
            content: "flowchart LR\n  A-->B".to_string(),
            level: DiagramLevel::System,
            diagram_type: "flowchart".to_string(),
            label: "test".to_string(),
        };
        let cloned = diag.clone();
        assert_eq!(cloned.content, diag.content);
        assert_eq!(cloned.level, diag.level);
        assert_eq!(cloned.diagram_type, diag.diagram_type);
        assert_eq!(cloned.label, diag.label);
    }
}
