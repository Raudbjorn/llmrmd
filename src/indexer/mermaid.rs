//! Mermaid diagram extraction and type inference.
//!
//! Extracts mermaid content from:
//! - Dedicated `.mmd` / `.mermaid` files (entire content is diagram)
//! - Fenced `\`\`\`mermaid` blocks in markdown/source files

use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;
use tracing::{debug, warn};

use crate::config::{
    CHARS_PER_TOKEN, MAX_MERMAID_SCAN_BYTES, MERMAID_FILE_EXTENSIONS, MERMAID_SOURCE_EXTENSIONS,
};

/// Regex for ```mermaid ... ``` fenced blocks.
static MERMAID_FENCE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?si)```mermaid\s*\n(.*?)\n\s*```").expect("valid regex")
});

/// Regex for stable diagram IDs: `<!-- id: some-id -->` before a mermaid block.
static DIAGRAM_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<!--\s*id:\s*(\S+)\s*-->").expect("valid regex")
});

/// A raw extracted diagram before it becomes a DiagramRecord.
#[derive(Debug)]
pub struct RawDiagram {
    pub id: Option<String>,
    pub content: String,
}

/// Extract mermaid diagram(s) from a file.
///
/// Returns an empty vec if the file is not a mermaid-containing type,
/// is too large, or cannot be read.
pub fn extract_from_file(abs_path: &Path) -> Vec<RawDiagram> {
    let ext = abs_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();

    let is_mermaid_file = MERMAID_FILE_EXTENSIONS.contains(ext.as_str());
    let is_source_file = MERMAID_SOURCE_EXTENSIONS.contains(ext.as_str());

    if !is_mermaid_file && !is_source_file {
        return Vec::new();
    }

    // Size guard
    let size = match std::fs::metadata(abs_path) {
        Ok(m) => m.len(),
        Err(_) => return Vec::new(),
    };

    if size > MAX_MERMAID_SCAN_BYTES {
        debug!(path = %abs_path.display(), size, "Skipping mermaid scan: file too large");
        return Vec::new();
    }

    let content = match std::fs::read_to_string(abs_path) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %abs_path.display(), error = %e, "Could not read file for mermaid extraction");
            return Vec::new();
        }
    };

    if is_mermaid_file {
        // Entire file is a diagram
        let trimmed = content.trim().to_string();
        if trimmed.is_empty() {
            return Vec::new();
        }
        // Check for ID comment at top
        let id = DIAGRAM_ID_RE
            .captures(&trimmed)
            .map(|c| c[1].to_string());
        return vec![RawDiagram {
            id,
            content: trimmed,
        }];
    }

    // Extract fenced blocks
    let mut results = Vec::new();
    let full_text = &content;

    for cap in MERMAID_FENCE_RE.captures_iter(full_text) {
        let diagram_content = cap[1].to_string();
        if diagram_content.trim().is_empty() {
            continue;
        }

        // Look for an ID comment just before this match
        let match_start = cap.get(0).map(|m| m.start()).unwrap_or(0);
        let before = &full_text[..match_start];
        let id = before
            .lines()
            .rev()
            .take(3) // check up to 3 lines before
            .find_map(|line| {
                DIAGRAM_ID_RE
                    .captures(line)
                    .map(|c| c[1].to_string())
            });

        results.push(RawDiagram {
            id,
            content: diagram_content,
        });
    }

    results
}

/// Infer the mermaid diagram type from the first meaningful line.
pub fn infer_type(content: &str) -> &'static str {
    for line in content.lines() {
        let stripped = line.trim().to_lowercase();
        if stripped.is_empty() || stripped.starts_with("%%") {
            continue;
        }

        if stripped.starts_with("graph ") || stripped.starts_with("graph\t") {
            return "flowchart";
        }

        let type_prefixes: &[(&str, &str)] = &[
            ("flowchart", "flowchart"),
            ("sequencediagram", "sequence"),
            ("classdiagram", "classDiagram"),
            ("erdiagram", "erDiagram"),
            ("statediagram", "stateDiagram"),
            ("gantt", "gantt"),
            ("pie", "pie"),
            ("gitgraph", "gitGraph"),
            ("c4", "c4"),
            ("mindmap", "mindmap"),
            ("timeline", "timeline"),
            ("journey", "journey"),
            ("sankey", "sankey"),
            ("xychart", "xyChart"),
            ("block", "block"),
            ("architecture", "architecture"),
        ];

        for (prefix, diagram_type) in type_prefixes {
            if stripped.starts_with(prefix) {
                return diagram_type;
            }
        }

        return "unknown";
    }

    "unknown"
}

/// Strip comments and excess whitespace from a mermaid diagram.
pub fn minify(diagram: &str) -> String {
    diagram
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("%%"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rough token estimate: `len / 4`, minimum 1.
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() / CHARS_PER_TOKEN).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn infer_type_flowchart() {
        assert_eq!(infer_type("flowchart TD\n  A-->B"), "flowchart");
        assert_eq!(infer_type("graph LR\n  A-->B"), "flowchart");
    }

    #[test]
    fn infer_type_sequence() {
        assert_eq!(infer_type("sequenceDiagram\n  A->>B: hello"), "sequence");
    }

    #[test]
    fn infer_type_er_diagram() {
        assert_eq!(infer_type("erDiagram\n  USER ||--o{ ORDER : places"), "erDiagram");
    }

    #[test]
    fn infer_type_state_diagram() {
        assert_eq!(infer_type("stateDiagram-v2\n  [*] --> Active"), "stateDiagram");
    }

    #[test]
    fn infer_type_pie() {
        assert_eq!(infer_type("pie title Pets\n  \"Dogs\" : 386"), "pie");
    }

    #[test]
    fn infer_type_with_comments() {
        assert_eq!(infer_type("%% this is a comment\nflowchart LR\n  A-->B"), "flowchart");
    }

    #[test]
    fn infer_type_empty() {
        assert_eq!(infer_type(""), "unknown");
    }

    #[test]
    fn infer_type_unknown() {
        assert_eq!(infer_type("randomGarbage"), "unknown");
    }

    #[test]
    fn minify_strips_comments_and_whitespace() {
        let input = "  flowchart LR\n  %% comment\n  A --> B\n  \n  B --> C  ";
        let result = minify(input);
        assert_eq!(result, "flowchart LR\nA --> B\nB --> C");
    }

    #[test]
    fn minify_empty() {
        assert_eq!(minify(""), "");
        assert_eq!(minify("  \n  \n  "), "");
    }

    #[test]
    fn estimate_tokens_basic() {
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars / 4 = 1
        assert_eq!(estimate_tokens("abcdefgh"), 2); // 8 / 4 = 2
    }

    #[test]
    fn estimate_tokens_minimum() {
        assert_eq!(estimate_tokens("ab"), 1); // 2 / 4 = 0, min 1
        assert_eq!(estimate_tokens(""), 1); // 0 / 4 = 0, min 1
    }

    #[test]
    fn extract_from_mmd_file() {
        let dir = tempfile::tempdir().unwrap();
        let mmd_path = dir.path().join("test.mmd");
        std::fs::write(&mmd_path, "flowchart LR\n  A-->B").unwrap();

        let results = extract_from_file(&mmd_path);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("flowchart LR"));
        assert!(results[0].id.is_none());
    }

    #[test]
    fn extract_from_mmd_file_with_id() {
        let dir = tempfile::tempdir().unwrap();
        let mmd_path = dir.path().join("test.mmd");
        std::fs::write(&mmd_path, "<!-- id: system-arch -->\nflowchart LR\n  A-->B").unwrap();

        let results = extract_from_file(&mmd_path);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_deref(), Some("system-arch"));
    }

    #[test]
    fn extract_fenced_blocks_from_md() {
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("test.md");
        let content = "# Header\n\nSome text\n\n```mermaid\nflowchart LR\n  A-->B\n```\n\nMore text\n\n```mermaid\nerDiagram\n  USER ||--o{ ORDER : places\n```\n";
        std::fs::write(&md_path, content).unwrap();

        let results = extract_from_file(&md_path);
        assert_eq!(results.len(), 2);
        assert!(results[0].content.contains("flowchart"));
        assert!(results[1].content.contains("erDiagram"));
    }

    #[test]
    fn extract_fenced_block_with_id_comment() {
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("test.md");
        let content = "# Header\n\n<!-- id: my-diagram -->\n```mermaid\nflowchart LR\n  A-->B\n```\n";
        std::fs::write(&md_path, content).unwrap();

        let results = extract_from_file(&md_path);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_deref(), Some("my-diagram"));
    }

    #[test]
    fn extract_skips_non_mermaid_files() {
        let dir = tempfile::tempdir().unwrap();
        let rs_path = dir.path().join("test.rs");
        std::fs::write(&rs_path, "fn main() {}").unwrap();

        let results = extract_from_file(&rs_path);
        assert!(results.is_empty());
    }

    #[test]
    fn extract_skips_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("large.md");
        let mut f = std::fs::File::create(&md_path).unwrap();
        // Write more than MAX_MERMAID_SCAN_BYTES
        for _ in 0..((MAX_MERMAID_SCAN_BYTES / 100) + 1) {
            f.write_all(&[b'x'; 100]).unwrap();
        }
        f.write_all(b"\n```mermaid\nflowchart LR\n  A-->B\n```\n").unwrap();

        let results = extract_from_file(&md_path);
        assert!(results.is_empty());
    }

    #[test]
    fn extract_empty_mmd_file() {
        let dir = tempfile::tempdir().unwrap();
        let mmd_path = dir.path().join("empty.mmd");
        std::fs::write(&mmd_path, "").unwrap();

        let results = extract_from_file(&mmd_path);
        assert!(results.is_empty());
    }
}
