//! Semantic tagging for diagrams in planning context.
//!
//! Wrapping diagrams in descriptive XML tags helps LLMs distinguish
//! diagram content from instructions and use the graph to verify relationships.

/// Tag type based on diagram purpose and scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramTag {
    SystemArchitecture,
    ContainerArchitecture,
    ClassDetail,
    SequenceFlow,
    EntityRelationship,
    StateTransition,
    DataFlow,
    GitHistory,
}

impl DiagramTag {
    /// Infer the appropriate tag from diagram type and scope.
    pub fn from_diagram(diagram_type: &str, scope: &str) -> Self {
        match (diagram_type, scope) {
            (_, "root") | (_, "system") => DiagramTag::SystemArchitecture,
            ("erDiagram", _) => DiagramTag::EntityRelationship,
            ("sequenceDiagram" | "sequence", _) => DiagramTag::SequenceFlow,
            ("stateDiagram" | "stateDiagram-v2", _) => DiagramTag::StateTransition,
            ("classDiagram", _) => DiagramTag::ClassDetail,
            ("gitGraph", _) => DiagramTag::GitHistory,
            ("flowchart", _) => DiagramTag::ContainerArchitecture,
            _ => DiagramTag::DataFlow,
        }
    }

    /// Get the XML-like tag name.
    pub fn tag_name(&self) -> &'static str {
        match self {
            DiagramTag::SystemArchitecture => "system_architecture_diagram",
            DiagramTag::ContainerArchitecture => "container_architecture_diagram",
            DiagramTag::ClassDetail => "class_detail_diagram",
            DiagramTag::SequenceFlow => "sequence_flow_diagram",
            DiagramTag::EntityRelationship => "entity_relationship_diagram",
            DiagramTag::StateTransition => "state_transition_diagram",
            DiagramTag::DataFlow => "data_flow_diagram",
            DiagramTag::GitHistory => "git_history_diagram",
        }
    }

    /// Priority for context injection (lower = higher priority, always included first).
    pub fn priority(&self) -> u8 {
        match self {
            DiagramTag::SystemArchitecture => 0,
            DiagramTag::ContainerArchitecture => 1,
            DiagramTag::EntityRelationship => 2,
            DiagramTag::SequenceFlow => 3,
            DiagramTag::ClassDetail => 4,
            DiagramTag::StateTransition => 5,
            DiagramTag::DataFlow => 6,
            DiagramTag::GitHistory => 7,
        }
    }
}

/// Metadata for diagram tags.
#[derive(Debug, Clone, Default)]
pub struct TagMetadata {
    pub scope: String,
    pub source: String,
    pub tokens: usize,
}

/// Wrap a diagram in semantic XML tags.
pub fn wrap_tagged(content: &str, tag: DiagramTag, metadata: &TagMetadata) -> String {
    let tag_name = tag.tag_name();
    let mut attrs = Vec::new();

    if !metadata.scope.is_empty() {
        attrs.push(format!("scope=\"{}\"", metadata.scope));
    }
    if !metadata.source.is_empty() {
        attrs.push(format!("source=\"{}\"", metadata.source));
    }
    if metadata.tokens > 0 {
        attrs.push(format!("tokens=\"{}\"", metadata.tokens));
    }

    let attr_str = if attrs.is_empty() {
        String::new()
    } else {
        format!(" {}", attrs.join(" "))
    };

    format!("<{tag_name}{attr_str}>\n```mermaid\n{content}\n```\n</{tag_name}>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_from_root_flowchart() {
        let tag = DiagramTag::from_diagram("flowchart", "root");
        assert_eq!(tag.tag_name(), "system_architecture_diagram");
    }

    #[test]
    fn tag_from_system_scope() {
        let tag = DiagramTag::from_diagram("flowchart", "system");
        assert_eq!(tag.tag_name(), "system_architecture_diagram");
    }

    #[test]
    fn tag_from_er_diagram() {
        let tag = DiagramTag::from_diagram("erDiagram", "web");
        assert_eq!(tag.tag_name(), "entity_relationship_diagram");
    }

    #[test]
    fn tag_from_sequence() {
        let tag = DiagramTag::from_diagram("sequenceDiagram", "api");
        assert_eq!(tag.tag_name(), "sequence_flow_diagram");
    }

    #[test]
    fn tag_from_sequence_short_name() {
        // The indexer's infer_type returns "sequence" not "sequenceDiagram"
        let tag = DiagramTag::from_diagram("sequence", "api");
        assert_eq!(tag.tag_name(), "sequence_flow_diagram");
    }

    #[test]
    fn tag_from_state_diagram() {
        let tag = DiagramTag::from_diagram("stateDiagram", "core");
        assert_eq!(tag.tag_name(), "state_transition_diagram");
    }

    #[test]
    fn tag_from_state_diagram_v2() {
        let tag = DiagramTag::from_diagram("stateDiagram-v2", "core");
        assert_eq!(tag.tag_name(), "state_transition_diagram");
    }

    #[test]
    fn tag_from_class_diagram() {
        let tag = DiagramTag::from_diagram("classDiagram", "models");
        assert_eq!(tag.tag_name(), "class_detail_diagram");
    }

    #[test]
    fn tag_from_git_graph() {
        let tag = DiagramTag::from_diagram("gitGraph", "infra");
        assert_eq!(tag.tag_name(), "git_history_diagram");
    }

    #[test]
    fn tag_from_non_root_flowchart() {
        let tag = DiagramTag::from_diagram("flowchart", "web");
        assert_eq!(tag.tag_name(), "container_architecture_diagram");
    }

    #[test]
    fn tag_from_unknown_type() {
        let tag = DiagramTag::from_diagram("pie", "analytics");
        assert_eq!(tag.tag_name(), "data_flow_diagram");
    }

    #[test]
    fn wrap_tagged_output() {
        let meta = TagMetadata {
            scope: "web".to_string(),
            source: "web/flow.mmd".to_string(),
            tokens: 50,
        };
        let result = wrap_tagged("flowchart LR\n  A-->B", DiagramTag::ContainerArchitecture, &meta);
        assert!(result.starts_with("<container_architecture_diagram"));
        assert!(result.contains("scope=\"web\""));
        assert!(result.contains("source=\"web/flow.mmd\""));
        assert!(result.contains("tokens=\"50\""));
        assert!(result.contains("```mermaid"));
        assert!(result.contains("flowchart LR\n  A-->B"));
        assert!(result.ends_with("</container_architecture_diagram>"));
    }

    #[test]
    fn wrap_tagged_no_metadata() {
        let meta = TagMetadata::default();
        let result = wrap_tagged("flowchart LR\n  A-->B", DiagramTag::SystemArchitecture, &meta);
        assert!(result.starts_with("<system_architecture_diagram>"));
        assert!(!result.contains("scope="));
        assert!(!result.contains("source="));
        assert!(!result.contains("tokens="));
    }

    #[test]
    fn wrap_tagged_partial_metadata() {
        let meta = TagMetadata {
            scope: "admin".to_string(),
            source: String::new(),
            tokens: 0,
        };
        let result = wrap_tagged("erDiagram\n  A ||--o{ B : has", DiagramTag::EntityRelationship, &meta);
        assert!(result.contains("scope=\"admin\""));
        assert!(!result.contains("source="));
        assert!(!result.contains("tokens="));
    }

    #[test]
    fn priorities_system_first() {
        assert!(DiagramTag::SystemArchitecture.priority() < DiagramTag::ContainerArchitecture.priority());
        assert!(DiagramTag::ContainerArchitecture.priority() < DiagramTag::ClassDetail.priority());
    }

    #[test]
    fn priorities_monotonic() {
        let tags = [
            DiagramTag::SystemArchitecture,
            DiagramTag::ContainerArchitecture,
            DiagramTag::EntityRelationship,
            DiagramTag::SequenceFlow,
            DiagramTag::ClassDetail,
            DiagramTag::StateTransition,
            DiagramTag::DataFlow,
            DiagramTag::GitHistory,
        ];
        for window in tags.windows(2) {
            assert!(
                window[0].priority() < window[1].priority(),
                "{:?} should have lower priority than {:?}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn tag_equality() {
        assert_eq!(DiagramTag::SystemArchitecture, DiagramTag::SystemArchitecture);
        assert_ne!(DiagramTag::SystemArchitecture, DiagramTag::DataFlow);
    }

    #[test]
    fn tag_clone() {
        let tag = DiagramTag::SequenceFlow;
        let cloned = tag;
        assert_eq!(tag, cloned);
    }
}
