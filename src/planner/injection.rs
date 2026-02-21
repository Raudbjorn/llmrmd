//! Context-aware diagram injection for planning prompts.
//!
//! Follows the hierarchy: always include system-level diagrams;
//! inject container/class diagrams only when editing within that scope.

use super::tags::{wrap_tagged, DiagramTag, TagMetadata};
use super::types::ManifestEntry;

/// A diagram prepared for injection with its tag and priority.
#[derive(Debug)]
pub struct TaggedDiagram {
    pub entry: ManifestEntry,
    pub content: String,
    pub tag: DiagramTag,
    pub priority: u8,
}

/// Select and order diagrams for context injection.
///
/// Strategy:
/// 1. System-level diagrams are always included (priority 0)
/// 2. Container-level diagrams for matching scopes (priority 1)
/// 3. Class-level diagrams only if scope exactly matches (priority 4+)
/// 4. Respect token budget — system diagrams are never dropped
pub fn select_diagrams_hierarchical(
    manifest: &[ManifestEntry],
    scopes: &[String],
    budget_remaining: usize,
    read_content: impl Fn(&str) -> String,
) -> Vec<TaggedDiagram> {
    let mut candidates: Vec<TaggedDiagram> = Vec::new();

    for entry in manifest {
        let tag = DiagramTag::from_diagram(&entry.diagram_type, &entry.scope);
        let priority = tag.priority();

        // System-level: always include
        // Scope-relevant: include if scope matches
        let should_include =
            entry.scope == "root" || scopes.contains(&entry.scope) || priority == 0;

        if !should_include {
            continue;
        }

        let content = read_content(&entry.source);
        if content.is_empty() {
            continue;
        }

        candidates.push(TaggedDiagram {
            entry: entry.clone(),
            content,
            tag,
            priority,
        });
    }

    // Sort by priority (system first), then by token cost (smaller first)
    candidates.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(a.entry.tokens_est.cmp(&b.entry.tokens_est))
    });

    // Apply budget
    let mut selected = Vec::new();
    let mut tokens_used = 0;

    for diag in candidates {
        if tokens_used + diag.entry.tokens_est > budget_remaining && !selected.is_empty() {
            // Never skip system diagrams even if over budget
            if diag.priority > 0 {
                continue;
            }
        }
        tokens_used += diag.entry.tokens_est;
        selected.push(diag);
    }

    selected
}

/// Render tagged diagrams into the planning prompt.
pub fn render_tagged_diagrams(diagrams: &[TaggedDiagram]) -> String {
    let mut sections = Vec::new();

    let mut current_section: Option<&'static str> = None;

    for diag in diagrams {
        let section_name = match diag.tag {
            DiagramTag::SystemArchitecture => "System Architecture (current state)",
            DiagramTag::ContainerArchitecture => "Container Architecture",
            DiagramTag::ClassDetail => "Class Details",
            DiagramTag::EntityRelationship => "Entity Relationships",
            DiagramTag::SequenceFlow => "Sequence Flows",
            DiagramTag::StateTransition => "State Transitions",
            DiagramTag::GitHistory => "Git History",
            DiagramTag::DataFlow => "Related Diagrams",
        };

        if current_section != Some(section_name) {
            sections.push(format!("\n## {section_name}\n"));
            current_section = Some(section_name);
        }

        let meta = TagMetadata {
            scope: diag.entry.scope.clone(),
            source: diag.entry.source.clone(),
            tokens: diag.entry.tokens_est,
        };

        sections.push(wrap_tagged(&diag.content, diag.tag, &meta));
        sections.push(String::new());
    }

    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, scope: &str, dtype: &str, tokens: usize) -> ManifestEntry {
        ManifestEntry {
            id: id.to_string(),
            source: format!("{scope}/{id}.mmd"),
            scope: scope.to_string(),
            diagram_type: dtype.to_string(),
            tokens_est: tokens,
        }
    }

    #[test]
    fn hierarchical_selection_system_first() {
        let manifest = vec![
            make_entry("sys", "root", "flowchart", 100),
            make_entry("web-detail", "web", "classDiagram", 200),
            make_entry("web-flow", "web", "flowchart", 150),
        ];

        let selected = select_diagrams_hierarchical(
            &manifest,
            &["web".to_string()],
            1000,
            |_| "flowchart LR\n  A-->B".to_string(),
        );

        assert!(!selected.is_empty());
        assert_eq!(selected[0].entry.scope, "root"); // System first
        assert_eq!(selected[0].priority, 0);
    }

    #[test]
    fn hierarchical_selection_includes_scope_diagrams() {
        let manifest = vec![
            make_entry("sys", "root", "flowchart", 100),
            make_entry("web-flow", "web", "flowchart", 150),
            make_entry("admin-flow", "admin", "flowchart", 150),
        ];

        let selected = select_diagrams_hierarchical(
            &manifest,
            &["web".to_string()],
            1000,
            |_| "flowchart LR\n  A-->B".to_string(),
        );

        // Should include root + web, but not admin
        assert_eq!(selected.len(), 2);
        let scopes: Vec<&str> = selected.iter().map(|d| d.entry.scope.as_str()).collect();
        assert!(scopes.contains(&"root"));
        assert!(scopes.contains(&"web"));
        assert!(!scopes.contains(&"admin"));
    }

    #[test]
    fn hierarchical_selection_respects_budget() {
        let manifest = vec![
            make_entry("sys", "root", "flowchart", 100),
            make_entry("big", "web", "classDiagram", 5000),
        ];

        let selected = select_diagrams_hierarchical(
            &manifest,
            &["web".to_string()],
            200,
            |_| "content".to_string(),
        );

        assert_eq!(selected.len(), 1); // Only system, big one skipped
        assert_eq!(selected[0].entry.scope, "root");
    }

    #[test]
    fn hierarchical_selection_always_includes_system() {
        let manifest = vec![make_entry("sys", "root", "flowchart", 500)];

        // Even with budget=0, system diagram should be included
        // because we never skip priority 0 when it's the first
        let selected = select_diagrams_hierarchical(
            &manifest,
            &[],
            0,
            |_| "flowchart LR\n  A-->B".to_string(),
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].entry.scope, "root");
    }

    #[test]
    fn hierarchical_selection_empty_content_skipped() {
        let manifest = vec![
            make_entry("sys", "root", "flowchart", 100),
            make_entry("empty", "web", "flowchart", 50),
        ];

        let selected = select_diagrams_hierarchical(
            &manifest,
            &["web".to_string()],
            1000,
            |source| {
                if source.contains("empty") {
                    String::new()
                } else {
                    "flowchart LR\n  A-->B".to_string()
                }
            },
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].entry.id, "sys");
    }

    #[test]
    fn hierarchical_selection_sorts_by_priority_then_tokens() {
        let manifest = vec![
            make_entry("big-class", "web", "classDiagram", 300),
            make_entry("small-flow", "web", "flowchart", 100),
            make_entry("sys", "root", "flowchart", 200),
        ];

        let selected = select_diagrams_hierarchical(
            &manifest,
            &["web".to_string()],
            1000,
            |_| "content".to_string(),
        );

        assert_eq!(selected.len(), 3);
        // System first (priority 0)
        assert_eq!(selected[0].entry.scope, "root");
        // Then container (priority 1) before class (priority 4)
        assert_eq!(selected[1].tag, DiagramTag::ContainerArchitecture);
        assert_eq!(selected[2].tag, DiagramTag::ClassDetail);
    }

    #[test]
    fn render_tagged_includes_xml_tags() {
        let manifest = vec![make_entry("sys", "root", "flowchart", 50)];
        let selected = select_diagrams_hierarchical(
            &manifest,
            &[],
            1000,
            |_| "flowchart LR\n  A-->B".to_string(),
        );

        let rendered = render_tagged_diagrams(&selected);
        assert!(rendered.contains("<system_architecture_diagram"));
        assert!(rendered.contains("</system_architecture_diagram>"));
        assert!(rendered.contains("```mermaid"));
    }

    #[test]
    fn render_tagged_groups_by_section() {
        let diagrams = vec![
            TaggedDiagram {
                entry: make_entry("sys", "root", "flowchart", 50),
                content: "flowchart LR\n  A-->B".to_string(),
                tag: DiagramTag::SystemArchitecture,
                priority: 0,
            },
            TaggedDiagram {
                entry: make_entry("web-flow", "web", "flowchart", 80),
                content: "flowchart LR\n  C-->D".to_string(),
                tag: DiagramTag::ContainerArchitecture,
                priority: 1,
            },
        ];

        let rendered = render_tagged_diagrams(&diagrams);
        assert!(rendered.contains("## System Architecture (current state)"));
        assert!(rendered.contains("## Container Architecture"));
    }

    #[test]
    fn render_tagged_empty_input() {
        let rendered = render_tagged_diagrams(&[]);
        assert!(rendered.is_empty() || rendered.trim().is_empty());
    }

    #[test]
    fn render_tagged_includes_metadata() {
        let diagrams = vec![TaggedDiagram {
            entry: make_entry("sys", "root", "flowchart", 50),
            content: "flowchart LR\n  A-->B".to_string(),
            tag: DiagramTag::SystemArchitecture,
            priority: 0,
        }];

        let rendered = render_tagged_diagrams(&diagrams);
        assert!(rendered.contains("scope=\"root\""));
        assert!(rendered.contains("source=\"root/sys.mmd\""));
        assert!(rendered.contains("tokens=\"50\""));
    }
}
