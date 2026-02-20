//! Post-validation of plans returned by the LLM.
//!
//! Checks plans against loaded context to catch hallucinated paths,
//! unknown domains, and invalid diagram references.

use tracing::warn;

use super::types::Plan;
use crate::indexer::types::FileRecord;
use crate::planner::types::ManifestEntry;

/// Validation result for a plan.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Warnings that don't block the plan but should be surfaced.
    pub warnings: Vec<String>,
    /// Hard errors that indicate the plan is unusable.
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate a plan against the known file index and manifest.
///
/// Checks:
/// 1. Files referenced for "modify" or "delete" actually exist in the index
/// 2. Diagram IDs in diagram_changes refer to known manifested diagrams
/// 3. Affected boundaries refer to known domains
/// 4. Context pointers reference known files
/// 5. Files marked "create" don't already exist (soft warning)
pub fn validate_plan(
    plan: &Plan,
    files: &[FileRecord],
    manifest: &[ManifestEntry],
) -> ValidationResult {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let known_paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let known_domains: Vec<&str> = files
        .iter()
        .map(|f| f.domain.as_str())
        .chain(manifest.iter().map(|m| m.scope.as_str()))
        .collect();
    let known_diagram_ids: Vec<&str> = manifest.iter().map(|m| m.id.as_str()).collect();

    // 1. Validate file changes
    for (i, fc) in plan.files_to_change.iter().enumerate() {
        match fc.action {
            super::types::FileAction::Modify | super::types::FileAction::Delete => {
                if !known_paths.contains(&fc.path.as_str()) {
                    errors.push(format!(
                        "Step {}: {} references '{}' which is not in the file index",
                        i + 1,
                        if fc.action == super::types::FileAction::Modify {
                            "modify"
                        } else {
                            "delete"
                        },
                        fc.path
                    ));
                }
            }
            super::types::FileAction::Create => {
                if known_paths.contains(&fc.path.as_str()) {
                    warnings.push(format!(
                        "Step {}: create '{}' but file already exists in index",
                        i + 1,
                        fc.path
                    ));
                }
            }
            super::types::FileAction::Move => {
                // Move source should exist
                if !known_paths.contains(&fc.path.as_str()) {
                    warnings.push(format!(
                        "Step {}: move '{}' but source not found in index",
                        i + 1,
                        fc.path
                    ));
                }
            }
        }

        // Validate depends_on references
        for &dep in &fc.depends_on {
            if dep >= plan.files_to_change.len() {
                errors.push(format!(
                    "Step {}: depends_on[{}] is out of range (only {} steps)",
                    i + 1,
                    dep,
                    plan.files_to_change.len()
                ));
            }
        }
    }

    // 2. Validate diagram changes
    for dc in &plan.diagram_changes {
        if dc.action == super::types::DiagramAction::Modify
            && !known_diagram_ids.contains(&dc.diagram_id.as_str())
        {
            warnings.push(format!(
                "Diagram '{}' marked for modify but not in manifest",
                dc.diagram_id
            ));
        }

        // Basic mermaid syntax check: should start with a known directive
        let trimmed = dc.proposed_mermaid.trim();
        let valid_starts = [
            "flowchart",
            "graph",
            "sequenceDiagram",
            "classDiagram",
            "stateDiagram",
            "erDiagram",
            "gantt",
            "pie",
            "gitgraph",
            "journey",
            "mindmap",
            "timeline",
            "quadrantChart",
            "sankey",
            "xychart",
            "block",
            "%%",
        ];
        if !valid_starts.iter().any(|s| trimmed.starts_with(s)) {
            warnings.push(format!(
                "Diagram '{}' mermaid content may be invalid (doesn't start with known directive)",
                dc.diagram_id
            ));
        }
    }

    // 3. Validate affected boundaries
    for boundary in &plan.affected_boundaries {
        if !known_domains.contains(&boundary.as_str()) && boundary != "root" {
            warnings.push(format!(
                "Affected boundary '{}' is not a known domain",
                boundary
            ));
        }
    }

    // 4. Validate context pointers
    for cp in &plan.context_pointers {
        if !known_paths.contains(&cp.path.as_str()) {
            warnings.push(format!(
                "Context pointer '{}' not found in file index",
                cp.path
            ));
        }
    }

    if !errors.is_empty() {
        warn!(
            errors = errors.len(),
            warnings = warnings.len(),
            "Plan validation found issues"
        );
    }

    ValidationResult { warnings, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::*;

    fn sample_files() -> Vec<FileRecord> {
        vec![
            FileRecord {
                path: "apps/web/src/index.ts".to_string(),
                file_type: "module".to_string(),
                domain: "web".to_string(),
                subdomain: "core".to_string(),
                claude_md: String::new(),
            },
            FileRecord {
                path: "apps/admin/src/index.ts".to_string(),
                file_type: "module".to_string(),
                domain: "admin".to_string(),
                subdomain: "core".to_string(),
                claude_md: String::new(),
            },
            FileRecord {
                path: "supabase/migrations/001.sql".to_string(),
                file_type: "migration".to_string(),
                domain: "supabase".to_string(),
                subdomain: "repositories".to_string(),
                claude_md: String::new(),
            },
        ]
    }

    fn sample_manifest() -> Vec<ManifestEntry> {
        vec![
            ManifestEntry {
                id: "root-arch".to_string(),
                source: "docs/arch.mmd".to_string(),
                scope: "root".to_string(),
                diagram_type: "flowchart".to_string(),
                tokens_est: 50,
                description: String::new(),
            },
            ManifestEntry {
                id: "web-flow".to_string(),
                source: "apps/web/flow.mmd".to_string(),
                scope: "web".to_string(),
                diagram_type: "flowchart".to_string(),
                tokens_est: 80,
                description: String::new(),
            },
        ]
    }

    #[test]
    fn valid_plan_passes() {
        let plan = Plan {
            title: "Add locations".to_string(),
            summary: "Create a locations table".to_string(),
            affected_boundaries: vec!["web".to_string(), "supabase".to_string()],
            context_pointers: vec![ContextPointer {
                path: "apps/web/src/index.ts".to_string(),
                reason: "entry point".to_string(),
            }],
            edge_changes: vec![],
            diagram_changes: vec![DiagramChange {
                diagram_id: "web-flow".to_string(),
                action: DiagramAction::Modify,
                proposed_mermaid: "flowchart LR\n  A-->B".to_string(),
            }],
            files_to_change: vec![FileChange {
                path: "apps/web/src/index.ts".to_string(),
                action: FileAction::Modify,
                description: "Add location import".to_string(),
                depends_on: vec![],
            }],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: Some(Uncertainty::Low),
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.is_valid());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn modify_nonexistent_file_is_error() {
        let plan = Plan {
            title: "Bad plan".to_string(),
            summary: "References nonexistent file".to_string(),
            affected_boundaries: vec!["web".to_string()],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![],
            files_to_change: vec![FileChange {
                path: "apps/web/src/nonexistent.ts".to_string(),
                action: FileAction::Modify,
                description: "Modify missing file".to_string(),
                depends_on: vec![],
            }],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(!result.is_valid());
        assert!(result.errors[0].contains("nonexistent.ts"));
    }

    #[test]
    fn create_existing_file_is_warning() {
        let plan = Plan {
            title: "Overwrite plan".to_string(),
            summary: "Creates file that exists".to_string(),
            affected_boundaries: vec!["web".to_string()],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![],
            files_to_change: vec![FileChange {
                path: "apps/web/src/index.ts".to_string(),
                action: FileAction::Create,
                description: "Create existing file".to_string(),
                depends_on: vec![],
            }],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.is_valid()); // warnings only, no errors
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("already exists"));
    }

    #[test]
    fn unknown_diagram_id_is_warning() {
        let plan = Plan {
            title: "Unknown diagram".to_string(),
            summary: "References unknown diagram".to_string(),
            affected_boundaries: vec![],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![DiagramChange {
                diagram_id: "nonexistent-diagram".to_string(),
                action: DiagramAction::Modify,
                proposed_mermaid: "flowchart LR\n  X-->Y".to_string(),
            }],
            files_to_change: vec![],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.is_valid());
        assert!(result.warnings.iter().any(|w| w.contains("nonexistent-diagram")));
    }

    #[test]
    fn unknown_boundary_is_warning() {
        let plan = Plan {
            title: "Unknown boundary".to_string(),
            summary: "References unknown domain".to_string(),
            affected_boundaries: vec!["nonexistent-domain".to_string()],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![],
            files_to_change: vec![],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.warnings.iter().any(|w| w.contains("nonexistent-domain")));
    }

    #[test]
    fn invalid_depends_on_is_error() {
        let plan = Plan {
            title: "Bad deps".to_string(),
            summary: "Invalid dependency reference".to_string(),
            affected_boundaries: vec![],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![],
            files_to_change: vec![FileChange {
                path: "new_file.ts".to_string(),
                action: FileAction::Create,
                description: "Create file".to_string(),
                depends_on: vec![99],
            }],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(!result.is_valid());
        assert!(result.errors[0].contains("out of range"));
    }

    #[test]
    fn invalid_mermaid_syntax_is_warning() {
        let plan = Plan {
            title: "Bad mermaid".to_string(),
            summary: "Invalid mermaid content".to_string(),
            affected_boundaries: vec![],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![DiagramChange {
                diagram_id: "new-diag".to_string(),
                action: DiagramAction::Create,
                proposed_mermaid: "this is not valid mermaid at all".to_string(),
            }],
            files_to_change: vec![],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.warnings.iter().any(|w| w.contains("may be invalid")));
    }

    #[test]
    fn root_boundary_is_always_valid() {
        let plan = Plan {
            title: "Root plan".to_string(),
            summary: "Affects root".to_string(),
            affected_boundaries: vec!["root".to_string()],
            context_pointers: vec![],
            edge_changes: vec![],
            diagram_changes: vec![],
            files_to_change: vec![],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.is_valid());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn context_pointer_unknown_path_is_warning() {
        let plan = Plan {
            title: "Unknown pointer".to_string(),
            summary: "Context pointer to unknown file".to_string(),
            affected_boundaries: vec![],
            context_pointers: vec![ContextPointer {
                path: "does/not/exist.ts".to_string(),
                reason: "some reason".to_string(),
            }],
            edge_changes: vec![],
            diagram_changes: vec![],
            files_to_change: vec![],
            files_to_avoid: vec![],
            edge_violations: vec![],
            risks: vec![],
            uncertainty: None,
        };

        let result = validate_plan(&plan, &sample_files(), &sample_manifest());
        assert!(result.warnings.iter().any(|w| w.contains("does/not/exist.ts")));
    }
}
