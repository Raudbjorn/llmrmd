//! Mermaid syntax validation and repair.
//!
//! Fixes common issues in Mermaid diagrams that cause rendering failures:
//! - Unescaped parentheses in node labels
//! - Invalid characters in node IDs
//! - Missing direction declarations
//! - Consecutive empty lines within diagram bodies

use once_cell::sync::Lazy;
use regex::Regex;
use tracing::warn;

/// Result of repairing a diagram.
#[derive(Debug)]
pub struct RepairResult {
    /// The repaired content (or original if no changes needed).
    pub content: String,
    /// Issues found and fixed.
    pub fixes: Vec<String>,
    /// Issues found but not fixable automatically.
    pub warnings: Vec<String>,
}

/// Validate and repair a Mermaid diagram.
pub fn repair(content: &str) -> RepairResult {
    let mut result = RepairResult {
        content: content.to_string(),
        fixes: Vec::new(),
        warnings: Vec::new(),
    };

    // Fix 1: Escape parentheses in node labels
    result.content = fix_unescaped_parens(&result.content, &mut result.fixes);

    // Fix 2: Add missing direction to flowcharts
    result.content = fix_missing_direction(&result.content, &mut result.fixes);

    // Fix 3: Fix invalid node IDs (spaces, special chars)
    result.content = fix_invalid_node_ids(&result.content, &mut result.fixes);

    // Fix 4: Remove consecutive empty lines within diagram body
    result.content = fix_empty_lines(&result.content, &mut result.fixes);

    // Warning: Check for excessive complexity
    check_complexity(&result.content, &mut result.warnings);

    result
}

static PAREN_IN_LABEL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\[([^\]]*)\(([^\)]*)\)([^\]]*)\]"#).unwrap());

fn fix_unescaped_parens(content: &str, fixes: &mut Vec<String>) -> String {
    let mut output = content.to_string();

    // Fix parentheses in square bracket labels: [text(with parens)] -> [text&#40;with parens&#41;]
    if PAREN_IN_LABEL.is_match(&output) {
        let fixed = PAREN_IN_LABEL.replace_all(&output, |caps: &regex::Captures| {
            format!("[{}&#40;{}&#41;{}]", &caps[1], &caps[2], &caps[3])
        });
        if fixed != output {
            fixes.push("Escaped parentheses in node labels".to_string());
            output = fixed.to_string();
        }
    }

    output
}

fn fix_missing_direction(content: &str, fixes: &mut Vec<String>) -> String {
    let trimmed = content.trim();

    // Check if it starts with "flowchart" without a direction
    if trimmed.starts_with("flowchart\n") || trimmed == "flowchart" {
        fixes.push("Added default direction (TB) to flowchart".to_string());
        return content.replacen("flowchart", "flowchart TB", 1);
    }

    // Check for "graph" without direction
    if trimmed.starts_with("graph\n") || trimmed == "graph" {
        fixes.push("Added default direction (TB) to graph".to_string());
        return content.replacen("graph", "graph TB", 1);
    }

    content.to_string()
}

static INVALID_ID: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s+([a-zA-Z_][\w]*(?:\s+\w+)+)(\[|\(|\{|-->|---)").unwrap()
});

fn fix_invalid_node_ids(content: &str, fixes: &mut Vec<String>) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut fixed_any = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip declaration lines and comments
        if trimmed.starts_with("flowchart")
            || trimmed.starts_with("graph")
            || trimmed.starts_with("sequenceDiagram")
            || trimmed.starts_with("classDiagram")
            || trimmed.starts_with("erDiagram")
            || trimmed.starts_with("stateDiagram")
            || trimmed.starts_with("gitGraph")
            || trimmed.starts_with("gantt")
            || trimmed.starts_with("pie")
            || trimmed.starts_with("mindmap")
            || trimmed.starts_with("timeline")
            || trimmed.starts_with("%%")
            || trimmed.is_empty()
        {
            lines.push(line.to_string());
            continue;
        }

        // Replace spaces in node IDs with underscores
        // This is a heuristic -- only fix obvious cases
        if let Some(caps) = INVALID_ID.captures(line) {
            let original_id = &caps[1];
            let fixed_id = original_id.replace(' ', "_");
            let fixed_line = line.replacen(original_id, &fixed_id, 1);
            lines.push(fixed_line);
            fixed_any = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if fixed_any {
        fixes.push("Fixed spaces in node IDs".to_string());
    }

    lines.join("\n")
}

fn fix_empty_lines(content: &str, fixes: &mut Vec<String>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result: Vec<&str> = Vec::new();
    let mut removed = 0;
    let mut in_diagram = false;

    for line in &lines {
        let trimmed = line.trim();

        if !in_diagram {
            // First non-empty line starts the diagram
            if !trimmed.is_empty() {
                in_diagram = true;
            }
            result.push(line);
            continue;
        }

        // Remove consecutive empty lines within diagram body
        if trimmed.is_empty()
            && result
                .last()
                .map(|l| l.trim().is_empty())
                .unwrap_or(false)
        {
            removed += 1;
            continue;
        }

        result.push(line);
    }

    if removed > 0 {
        fixes.push(format!("Removed {removed} consecutive empty lines"));
    }

    result.join("\n")
}

fn check_complexity(content: &str, warnings: &mut Vec<String>) {
    let line_count = content.lines().count();
    let arrow_count = content.matches("-->").count()
        + content.matches("---").count()
        + content.matches("-.->").count()
        + content.matches("==>").count();

    if line_count > 200 {
        warnings.push(format!(
            "Diagram has {line_count} lines -- consider breaking into smaller diagrams"
        ));
    }

    if arrow_count > 50 {
        warnings.push(format!(
            "Diagram has {arrow_count} connections -- may be hard to read (\"hairball\" diagram)"
        ));
    }

    // Check for likely node count
    static NODE_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^\s+\w+[\[\(\{]").unwrap());

    let node_count = content
        .lines()
        .filter(|l| NODE_PATTERN.is_match(l))
        .count();
    if node_count > 30 {
        warnings.push(format!(
            "Diagram has ~{node_count} nodes -- consider hierarchical decomposition"
        ));
    }
}

/// Log repair results at appropriate levels.
pub fn log_repair_results(source: &str, result: &RepairResult) {
    for fix in &result.fixes {
        tracing::info!(source, fix, "Mermaid repair applied");
    }
    for warning in &result.warnings {
        warn!(source, warning, "Mermaid complexity warning");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_missing_direction() {
        let input = "flowchart\n    A-->B";
        let result = repair(input);
        assert!(result.content.starts_with("flowchart TB"));
        assert!(!result.fixes.is_empty());
        assert!(result.fixes.iter().any(|f| f.contains("direction")));
    }

    #[test]
    fn repair_missing_direction_graph() {
        let input = "graph\n    A-->B";
        let result = repair(input);
        assert!(result.content.starts_with("graph TB"));
        assert!(result.fixes.iter().any(|f| f.contains("direction")));
    }

    #[test]
    fn repair_already_has_direction() {
        let input = "flowchart LR\n    A-->B";
        let result = repair(input);
        assert!(result.content.starts_with("flowchart LR"));
        // Should not add a fix for direction
        assert!(!result.fixes.iter().any(|f| f.contains("direction")));
    }

    #[test]
    fn repair_already_has_direction_td() {
        let input = "flowchart TD\n    A-->B";
        let result = repair(input);
        assert!(result.content.starts_with("flowchart TD"));
        assert!(!result.fixes.iter().any(|f| f.contains("direction")));
    }

    #[test]
    fn repair_unescaped_parens() {
        let input = "flowchart LR\n    A[Node (with parens)]-->B";
        let result = repair(input);
        assert!(result.content.contains("&#40;"));
        assert!(result.content.contains("&#41;"));
        assert!(
            result
                .fixes
                .iter()
                .any(|f| f.contains("parentheses"))
        );
    }

    #[test]
    fn repair_no_parens_in_round_brackets() {
        // Parentheses inside round bracket nodes are valid Mermaid syntax
        let input = "flowchart LR\n    A(Node text)-->B";
        let result = repair(input);
        // Should not modify round bracket content
        assert!(result.content.contains("A(Node text)"));
    }

    #[test]
    fn repair_consecutive_empty_lines() {
        let input = "flowchart LR\n    A-->B\n\n\n\n    C-->D";
        let result = repair(input);
        // Should not have more than one consecutive empty line
        assert!(!result.content.contains("\n\n\n"));
        // Content should still be there
        assert!(result.content.contains("A-->B"));
        assert!(result.content.contains("C-->D"));
    }

    #[test]
    fn repair_single_empty_line_preserved() {
        let input = "flowchart LR\n    A-->B\n\n    C-->D";
        let result = repair(input);
        // Single empty line should remain
        assert!(result.content.contains("A-->B\n\n    C-->D"));
        assert!(
            !result
                .fixes
                .iter()
                .any(|f| f.contains("empty lines"))
        );
    }

    #[test]
    fn complexity_warning_many_lines() {
        let mut input = "flowchart LR\n".to_string();
        for i in 0..250 {
            input.push_str(&format!("    N{i}-->N{}\n", i + 1));
        }
        let result = repair(&input);
        assert!(result.warnings.iter().any(|w| w.contains("lines")));
    }

    #[test]
    fn complexity_warning_many_connections() {
        let mut input = "flowchart LR\n".to_string();
        for i in 0..60 {
            input.push_str(&format!("    A{i}-->B{i}\n"));
        }
        let result = repair(&input);
        assert!(result.warnings.iter().any(|w| w.contains("connections")));
    }

    #[test]
    fn no_fixes_for_valid_diagram() {
        let input = "flowchart LR\n    A[Start]-->B[End]";
        let result = repair(input);
        assert!(result.fixes.is_empty());
    }

    #[test]
    fn no_warnings_for_small_diagram() {
        let input = "flowchart LR\n    A[Start]-->B[End]";
        let result = repair(input);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn repair_preserves_non_flowchart_types() {
        let input = "erDiagram\n    USER ||--o{ ORDER : places";
        let result = repair(input);
        assert!(result.content.starts_with("erDiagram"));
        assert!(result.fixes.is_empty());
    }

    #[test]
    fn repair_preserves_sequence_diagram() {
        let input = "sequenceDiagram\n    Alice->>Bob: Hello";
        let result = repair(input);
        assert!(result.content.starts_with("sequenceDiagram"));
        assert!(result.fixes.is_empty());
    }

    #[test]
    fn repair_empty_input() {
        let result = repair("");
        assert!(result.content.is_empty());
        assert!(result.fixes.is_empty());
    }

    #[test]
    fn repair_only_flowchart_keyword() {
        let result = repair("flowchart");
        assert_eq!(result.content.trim(), "flowchart TB");
    }

    #[test]
    fn repair_result_accumulates_multiple_fixes() {
        // Input with both missing direction and unescaped parens
        let input = "flowchart\n    A[Node (bad)]-->B";
        let result = repair(input);
        assert!(result.fixes.len() >= 2);
        assert!(result.fixes.iter().any(|f| f.contains("direction")));
        assert!(
            result
                .fixes
                .iter()
                .any(|f| f.contains("parentheses"))
        );
    }
}
