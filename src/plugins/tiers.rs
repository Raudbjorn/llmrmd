//! Hierarchical three-tier diagram management (C4-inspired).
//!
//! Manages the three abstraction levels for diagram generation:
//! - **System**: auto-generated from domain structure (highest level)
//! - **Container**: from plugin analysis or existing diagrams (mid-level)
//! - **Class**: detailed per-module diagrams (lowest level)
//!
//! The tier system enables context-aware diagram selection -- a high-level
//! planning task only needs System-tier diagrams, while debugging a specific
//! module pulls in Class-tier detail.

use super::DiagramLevel;

/// Generate a system-level architecture diagram from domain structure.
///
/// Produces a Mermaid flowchart showing domain nodes and their dependency edges.
/// Skips the "root" domain (which represents top-level files without a logical grouping).
///
/// # Arguments
///
/// * `domains` -- list of domain names discovered during indexing
/// * `domain_deps` -- directed edges `(from, to)` representing inter-domain dependencies
pub fn generate_system_diagram(
    domains: &[String],
    domain_deps: &[(String, String)],
) -> String {
    let mut lines = vec!["flowchart TB".to_string()];

    for domain in domains {
        if domain == "root" {
            continue;
        }
        let safe_id = sanitize_node_id(domain);
        lines.push(format!("    {safe_id}[{domain}]"));
    }

    for (from, to) in domain_deps {
        if from == "root" || to == "root" {
            continue;
        }
        let from_id = sanitize_node_id(from);
        let to_id = sanitize_node_id(to);
        if from_id != to_id {
            lines.push(format!("    {from_id} --> {to_id}"));
        }
    }

    lines.join("\n")
}

/// Return a semantic tag for the given diagram level.
///
/// These tags are used in manifest metadata to enable level-aware filtering
/// during context selection.
pub fn tier_tag(level: DiagramLevel) -> &'static str {
    match level {
        DiagramLevel::System => "system_architecture",
        DiagramLevel::Container => "container_architecture",
        DiagramLevel::Class => "class_detail",
    }
}

/// Return a human-readable description of the diagram level.
pub fn tier_description(level: DiagramLevel) -> &'static str {
    match level {
        DiagramLevel::System => "High-level domain/service relationships",
        DiagramLevel::Container => "Module-level interactions within a domain",
        DiagramLevel::Class => "Detailed class/function/trait internals",
    }
}

/// Map a diagram level to suggested context window usage.
///
/// This helps the planner decide how many tokens to allocate per tier.
pub fn tier_token_weight(level: DiagramLevel) -> f32 {
    match level {
        DiagramLevel::System => 0.15,    // Small, high-signal
        DiagramLevel::Container => 0.35, // Medium detail
        DiagramLevel::Class => 0.50,     // Largest, most granular
    }
}

/// Replace characters invalid in Mermaid node IDs with underscores.
fn sanitize_node_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- System diagram generation ----

    #[test]
    fn generate_system_diagram_basic() {
        let domains = vec![
            "web".to_string(),
            "api".to_string(),
            "shared".to_string(),
        ];
        let deps = vec![
            ("web".to_string(), "api".to_string()),
            ("web".to_string(), "shared".to_string()),
            ("api".to_string(), "shared".to_string()),
        ];

        let diagram = generate_system_diagram(&domains, &deps);

        assert!(diagram.starts_with("flowchart TB"));
        assert!(diagram.contains("web[web]"));
        assert!(diagram.contains("api[api]"));
        assert!(diagram.contains("shared[shared]"));
        assert!(diagram.contains("web --> api"));
        assert!(diagram.contains("web --> shared"));
        assert!(diagram.contains("api --> shared"));
    }

    #[test]
    fn generate_system_diagram_skips_root() {
        let domains = vec![
            "root".to_string(),
            "web".to_string(),
            "api".to_string(),
        ];
        let deps = vec![
            ("root".to_string(), "web".to_string()),
            ("web".to_string(), "api".to_string()),
        ];

        let diagram = generate_system_diagram(&domains, &deps);

        assert!(!diagram.contains("root[root]"));
        assert!(diagram.contains("web[web]"));
        // root->web edge should be skipped.
        assert!(!diagram.contains("root --> web"));
        assert!(diagram.contains("web --> api"));
    }

    #[test]
    fn generate_system_diagram_no_domains() {
        let diagram = generate_system_diagram(&[], &[]);
        assert_eq!(diagram, "flowchart TB");
    }

    #[test]
    fn generate_system_diagram_only_root() {
        let domains = vec!["root".to_string()];
        let diagram = generate_system_diagram(&domains, &[]);
        assert_eq!(diagram, "flowchart TB");
    }

    #[test]
    fn generate_system_diagram_no_self_loops() {
        let domains = vec!["web".to_string()];
        let deps = vec![("web".to_string(), "web".to_string())];

        let diagram = generate_system_diagram(&domains, &deps);

        assert!(!diagram.contains("web --> web"));
    }

    #[test]
    fn generate_system_diagram_special_chars_in_domain() {
        let domains = vec!["my-service".to_string(), "api-v2".to_string()];
        let deps = vec![("my-service".to_string(), "api-v2".to_string())];

        let diagram = generate_system_diagram(&domains, &deps);

        // Hyphens should be sanitized to underscores in IDs.
        assert!(diagram.contains("my_service[my-service]"));
        assert!(diagram.contains("api_v2[api-v2]"));
        assert!(diagram.contains("my_service --> api_v2"));
    }

    // ---- Tier tags ----

    #[test]
    fn tier_tag_values() {
        assert_eq!(tier_tag(DiagramLevel::System), "system_architecture");
        assert_eq!(tier_tag(DiagramLevel::Container), "container_architecture");
        assert_eq!(tier_tag(DiagramLevel::Class), "class_detail");
    }

    // ---- Tier descriptions ----

    #[test]
    fn tier_description_values() {
        assert!(tier_description(DiagramLevel::System).contains("domain"));
        assert!(tier_description(DiagramLevel::Container).contains("Module"));
        assert!(tier_description(DiagramLevel::Class).contains("class"));
    }

    // ---- Token weights ----

    #[test]
    fn tier_token_weights_sum_to_one() {
        let total = tier_token_weight(DiagramLevel::System)
            + tier_token_weight(DiagramLevel::Container)
            + tier_token_weight(DiagramLevel::Class);
        assert!((total - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tier_token_weights_ordering() {
        assert!(tier_token_weight(DiagramLevel::System) < tier_token_weight(DiagramLevel::Container));
        assert!(
            tier_token_weight(DiagramLevel::Container) < tier_token_weight(DiagramLevel::Class)
        );
    }

    // ---- Sanitize helper ----

    #[test]
    fn sanitize_node_id_basic() {
        assert_eq!(sanitize_node_id("hello"), "hello");
        assert_eq!(sanitize_node_id("foo-bar"), "foo_bar");
        assert_eq!(sanitize_node_id("a.b/c"), "a_b_c");
    }
}
