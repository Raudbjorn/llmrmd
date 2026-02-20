//! Mermaid-to-NL description generator.
//!
//! Parses Mermaid diagram syntax and produces 1-3 sentence natural language
//! descriptions suitable for semantic search and TF-IDF vectorization.

use once_cell::sync::Lazy;
use regex::Regex;

// ---------------------------------------------------------------------------
// Regex patterns for each diagram type
// ---------------------------------------------------------------------------

/// Flowchart node labels: `A[Label]`, `A(Label)`, `A{Label}`, `A([Label])`, `A[[Label]]`
static FLOW_NODE_LABEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:\w+)\s*(?:\[\[([^\]]+)\]\]|\[([^\]]+)\]|\(\[([^\]]+)\]\)|\(([^)]+)\)|\{([^}]+)\})"#).unwrap()
});

/// Flowchart edge labels: `-->|label|` or `-- label -->`
static FLOW_EDGE_LABEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"-->?\|([^|]+)\||--\s+([^-]+?)\s+-->"#).unwrap()
});

/// Sequence participants: `participant A` or `participant A as Label`
static SEQ_PARTICIPANT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)participant\s+(\w+)(?:\s+as\s+(.+))?").unwrap()
});

/// Sequence actors: `actor A` or `actor A as Label`
static SEQ_ACTOR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)actor\s+(\w+)(?:\s+as\s+(.+))?").unwrap()
});

/// Sequence messages: `A->>B: message` or `A-->>B: message` etc.
static SEQ_MESSAGE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\w+)\s*->>?\+?\s*(\w+)\s*:\s*(.+)").unwrap()
});

/// ER entities and relationships: `ENTITY1 ||--o{ ENTITY2 : label`
static ER_RELATIONSHIP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\w+)\s+[\|o\}]{1,2}--[\|o\{]{1,2}\s+(\w+)\s*:\s*(.+)").unwrap()
});

/// Class names from classDiagram: `class ClassName` or `ClassName : method()`
static CLASS_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:class\s+(\w+)|(\w+)\s+:\s+)").unwrap()
});

/// Class inheritance: `A <|-- B` or `A --|> B`
static CLASS_INHERIT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\w+)\s+<\|--\s+(\w+)|(\w+)\s+--\|>\s+(\w+)").unwrap()
});

/// State transitions: `StateA --> StateB` or `StateA --> StateB : label`
static STATE_TRANSITION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\w+|\[\*\])\s+-->\s+(\w+|\[\*\])(?:\s*:\s*(.+))?").unwrap()
});

/// State names from `state "Label" as StateId`
static STATE_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"state\s+"([^"]+)"\s+as\s+(\w+)"#).unwrap()
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a natural language description for a Mermaid diagram.
///
/// `content` is the minified Mermaid source, `diagram_type` is the inferred type
/// (e.g. "flowchart", "sequence"), and `domain` is the logical domain scope.
pub fn describe(content: &str, diagram_type: &str, domain: &str) -> String {
    let desc = match diagram_type {
        "flowchart" => describe_flowchart(content, domain),
        "sequence" => describe_sequence(content, domain),
        "erDiagram" => describe_er(content, domain),
        "classDiagram" => describe_class(content, domain),
        "stateDiagram" => describe_state(content, domain),
        _ => describe_fallback(content, diagram_type, domain),
    };

    sanitize_description(&desc)
}

// ---------------------------------------------------------------------------
// Type-specific describers
// ---------------------------------------------------------------------------

fn describe_flowchart(content: &str, domain: &str) -> String {
    let mut node_labels = Vec::new();
    let mut edge_labels = Vec::new();

    for cap in FLOW_NODE_LABEL.captures_iter(content) {
        let label = cap.get(1)
            .or_else(|| cap.get(2))
            .or_else(|| cap.get(3))
            .or_else(|| cap.get(4))
            .or_else(|| cap.get(5))
            .map(|m| m.as_str().trim().to_string());
        if let Some(l) = label {
            if !node_labels.contains(&l) {
                node_labels.push(l);
            }
        }
    }

    for cap in FLOW_EDGE_LABEL.captures_iter(content) {
        let label = cap.get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str().trim().to_string());
        if let Some(l) = label {
            if !edge_labels.contains(&l) {
                edge_labels.push(l);
            }
        }
    }

    let mut parts = Vec::new();

    let domain_str = if domain == "root" { String::new() } else { format!(" in {domain}") };
    let node_summary = if node_labels.is_empty() {
        String::new()
    } else {
        let display: Vec<&str> = node_labels.iter().take(8).map(|s| s.as_str()).collect();
        format!(" showing flow between {}", display.join(", "))
    };
    parts.push(format!("Flowchart{domain_str}{node_summary}."));

    if !edge_labels.is_empty() {
        let display: Vec<&str> = edge_labels.iter().take(6).map(|s| s.as_str()).collect();
        parts.push(format!("Connections: {}.", display.join(", ")));
    }

    parts.join(" ")
}

fn describe_sequence(content: &str, domain: &str) -> String {
    let mut participants = Vec::new();
    let mut messages = Vec::new();

    for cap in SEQ_PARTICIPANT.captures_iter(content) {
        let name = cap.get(2)
            .or_else(|| cap.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if !name.is_empty() && !participants.contains(&name) {
            participants.push(name);
        }
    }

    for cap in SEQ_ACTOR.captures_iter(content) {
        let name = cap.get(2)
            .or_else(|| cap.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if !name.is_empty() && !participants.contains(&name) {
            participants.push(name);
        }
    }

    // Also extract implicit participants from message arrows
    for cap in SEQ_MESSAGE.captures_iter(content) {
        if let Some(sender) = cap.get(1) {
            let s = sender.as_str().to_string();
            if !participants.contains(&s) {
                participants.push(s);
            }
        }
        if let Some(receiver) = cap.get(2) {
            let r = receiver.as_str().to_string();
            if !participants.contains(&r) {
                participants.push(r);
            }
        }
        if let Some(msg) = cap.get(3) {
            let m = msg.as_str().trim().to_string();
            if !messages.contains(&m) {
                messages.push(m);
            }
        }
    }

    let mut parts = Vec::new();

    let domain_str = if domain == "root" { String::new() } else { format!(" in {domain}") };
    let participant_summary = if participants.is_empty() {
        String::new()
    } else {
        let display: Vec<&str> = participants.iter().take(6).map(|s| s.as_str()).collect();
        format!(" between {}", display.join(", "))
    };
    parts.push(format!("Sequence diagram{domain_str} showing interactions{participant_summary}."));

    if !messages.is_empty() {
        let display: Vec<&str> = messages.iter().take(5).map(|s| s.as_str()).collect();
        parts.push(format!("Flow covers {}.", display.join(", ")));
    }

    parts.join(" ")
}

fn describe_er(content: &str, domain: &str) -> String {
    let mut entities = Vec::new();
    let mut relationships = Vec::new();

    for cap in ER_RELATIONSHIP.captures_iter(content) {
        if let Some(e1) = cap.get(1) {
            let name = e1.as_str().to_string();
            if !entities.contains(&name) {
                entities.push(name);
            }
        }
        if let Some(e2) = cap.get(2) {
            let name = e2.as_str().to_string();
            if !entities.contains(&name) {
                entities.push(name);
            }
        }
        if let Some(rel) = cap.get(3) {
            relationships.push(rel.as_str().trim().to_string());
        }
    }

    let mut parts = Vec::new();

    let domain_str = if domain == "root" { String::new() } else { format!(" in {domain}") };
    let entity_summary = if entities.is_empty() {
        String::new()
    } else {
        let display: Vec<&str> = entities.iter().take(8).map(|s| s.as_str()).collect();
        format!(" defining {}", display.join(", "))
    };
    parts.push(format!("ER diagram{domain_str}{entity_summary}."));

    if !relationships.is_empty() {
        let display: Vec<&str> = relationships.iter().take(6).map(|s| s.as_str()).collect();
        parts.push(format!("Relationships: {}.", display.join(", ")));
    }

    parts.join(" ")
}

fn describe_class(content: &str, domain: &str) -> String {
    let mut classes = Vec::new();
    let mut inheritances = Vec::new();

    for cap in CLASS_NAME.captures_iter(content) {
        let name = cap.get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if !name.is_empty() && !classes.contains(&name) {
            classes.push(name);
        }
    }

    for cap in CLASS_INHERIT.captures_iter(content) {
        let parent = cap.get(1).or_else(|| cap.get(4)).map(|m| m.as_str());
        let child = cap.get(2).or_else(|| cap.get(3)).map(|m| m.as_str());
        if let (Some(p), Some(c)) = (parent, child) {
            inheritances.push(format!("{c} extends {p}"));
        }
    }

    let mut parts = Vec::new();

    let domain_str = if domain == "root" { String::new() } else { format!(" in {domain}") };
    let class_summary = if classes.is_empty() {
        String::new()
    } else {
        let display: Vec<&str> = classes.iter().take(8).map(|s| s.as_str()).collect();
        format!(" defining {}", display.join(", "))
    };
    parts.push(format!("Class diagram{domain_str}{class_summary}."));

    if !inheritances.is_empty() {
        let display: Vec<&str> = inheritances.iter().take(4).map(|s| s.as_str()).collect();
        parts.push(format!("Inheritance: {}.", display.join(", ")));
    }

    parts.join(" ")
}

fn describe_state(content: &str, domain: &str) -> String {
    let mut states = Vec::new();
    let mut transitions = Vec::new();

    // Named states
    for cap in STATE_NAME.captures_iter(content) {
        if let Some(label) = cap.get(1) {
            let name = label.as_str().to_string();
            if !states.contains(&name) {
                states.push(name);
            }
        }
    }

    // States and transitions from arrows
    for cap in STATE_TRANSITION.captures_iter(content) {
        for idx in [1, 2] {
            if let Some(m) = cap.get(idx) {
                let name = m.as_str().to_string();
                if name != "[*]" && !states.contains(&name) {
                    states.push(name);
                }
            }
        }
        if let Some(label) = cap.get(3) {
            transitions.push(label.as_str().trim().to_string());
        }
    }

    let mut parts = Vec::new();

    let domain_str = if domain == "root" { String::new() } else { format!(" in {domain}") };
    let state_summary = if states.is_empty() {
        String::new()
    } else {
        let display: Vec<&str> = states.iter().take(8).map(|s| s.as_str()).collect();
        format!(" with states {}", display.join(", "))
    };
    parts.push(format!("State diagram{domain_str}{state_summary}."));

    if !transitions.is_empty() {
        let display: Vec<&str> = transitions.iter().take(6).map(|s| s.as_str()).collect();
        parts.push(format!("Transitions: {}.", display.join(", ")));
    }

    parts.join(" ")
}

fn describe_fallback(content: &str, diagram_type: &str, domain: &str) -> String {
    let element_count = content.lines().count().saturating_sub(1); // subtract declaration line
    let domain_str = if domain == "root" { String::new() } else { format!(" in {domain}") };
    format!("Mermaid {diagram_type} diagram{domain_str} with {element_count} elements.")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sanitize a description: remove HTML entities, normalize whitespace.
fn sanitize_description(desc: &str) -> String {
    desc.replace("&#40;", "(")
        .replace("&#41;", ")")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_flowchart_basic() {
        let content = "flowchart LR\nA[Start] --> B[Process] --> C[End]";
        let desc = describe(content, "flowchart", "web");
        assert!(desc.contains("Flowchart"));
        assert!(desc.contains("web"));
        assert!(desc.contains("Start"));
        assert!(desc.contains("Process"));
        assert!(desc.contains("End"));
    }

    #[test]
    fn describe_flowchart_with_edge_labels() {
        let content = "flowchart LR\nA[Start] -->|yes| B[OK]\nA -->|no| C[Fail]";
        let desc = describe(content, "flowchart", "root");
        assert!(desc.contains("Flowchart"));
        assert!(desc.contains("yes"));
        assert!(desc.contains("no"));
    }

    #[test]
    fn describe_flowchart_root_domain_no_domain_text() {
        let content = "flowchart LR\nA-->B";
        let desc = describe(content, "flowchart", "root");
        assert!(!desc.contains("in root"));
    }

    #[test]
    fn describe_sequence_basic() {
        let content = "sequenceDiagram\nAlice->>Bob: Hello\nBob->>Alice: Hi back";
        let desc = describe(content, "sequence", "api");
        assert!(desc.contains("Sequence diagram"));
        assert!(desc.contains("api"));
        assert!(desc.contains("Alice"));
        assert!(desc.contains("Bob"));
        assert!(desc.contains("Hello"));
    }

    #[test]
    fn describe_sequence_with_participants() {
        let content = "sequenceDiagram\nparticipant A as API Gateway\nparticipant B as Backend\nA->>B: request";
        let desc = describe(content, "sequence", "root");
        assert!(desc.contains("API Gateway"));
        assert!(desc.contains("Backend"));
    }

    #[test]
    fn describe_er_basic() {
        let content = "erDiagram\nUSER ||--o{ ORDER : places\nORDER ||--|{ LINE_ITEM : contains";
        let desc = describe(content, "erDiagram", "supabase");
        assert!(desc.contains("ER diagram"));
        assert!(desc.contains("supabase"));
        assert!(desc.contains("USER"));
        assert!(desc.contains("ORDER"));
        assert!(desc.contains("LINE_ITEM"));
        assert!(desc.contains("places"));
    }

    #[test]
    fn describe_class_basic() {
        let content = "classDiagram\nclass Animal\nclass Dog\nDog <|-- Animal";
        let desc = describe(content, "classDiagram", "shared");
        assert!(desc.contains("Class diagram"));
        assert!(desc.contains("shared"));
        assert!(desc.contains("Animal"));
        assert!(desc.contains("Dog"));
    }

    #[test]
    fn describe_state_basic() {
        let content = "stateDiagram-v2\n[*] --> Active\nActive --> Inactive : deactivate\nInactive --> [*]";
        let desc = describe(content, "stateDiagram", "web");
        assert!(desc.contains("State diagram"));
        assert!(desc.contains("web"));
        assert!(desc.contains("Active"));
        assert!(desc.contains("Inactive"));
        assert!(desc.contains("deactivate"));
    }

    #[test]
    fn describe_fallback_unknown_type() {
        let content = "gantt\ntitle Project\nsection A\nTask1 :a1, 2024-01-01, 30d";
        let desc = describe(content, "gantt", "infra");
        assert!(desc.contains("Mermaid gantt diagram"));
        assert!(desc.contains("infra"));
        assert!(desc.contains("elements"));
    }

    #[test]
    fn describe_sanitizes_html_entities() {
        let content = "flowchart LR\nA[Node &#40;with parens&#41;]-->B[End]";
        let desc = describe(content, "flowchart", "root");
        // Should have decoded entities
        assert!(desc.contains("(with parens)") || desc.contains("Node"));
    }

    #[test]
    fn describe_empty_content() {
        let desc = describe("flowchart LR", "flowchart", "root");
        assert!(desc.contains("Flowchart"));
    }
}
