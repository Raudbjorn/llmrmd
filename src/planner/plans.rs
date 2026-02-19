//! Plan persistence and management.
//!
//! Plans are stored as JSON files in `.claude/plans/`.
//! Each file is named `<slug>.json` where the slug is derived from
//! the change description and a timestamp.

use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::{info, warn};

use crate::api::types::{
    ContextSummary, PersistedPlan, PlanStatus, PlanToolResponse, Usage,
};
use crate::config::output_dir;
use crate::error::{self, Error, Result};
use crate::planner::types::PlannerContext;

/// Directory where plans are stored, relative to `.claude/`.
const PLANS_DIR: &str = "plans";

/// Get the plans directory path.
fn plans_dir(root: &Path) -> PathBuf {
    output_dir(root).join(PLANS_DIR)
}

/// Generate a URL-safe slug from a change description.
///
/// Converts to lowercase, replaces non-alphanumeric chars with hyphens,
/// collapses multiple hyphens, trims, and truncates to 50 chars.
pub fn slugify(description: &str) -> String {
    let slug: String = description
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse multiple hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // Trim trailing hyphen and truncate
    let trimmed = result.trim_end_matches('-');
    if trimmed.len() > 50 {
        // Find a clean break point
        let truncated = &trimmed[..50];
        truncated
            .rfind('-')
            .map(|i| &truncated[..i])
            .unwrap_or(truncated)
            .to_string()
    } else {
        trimmed.to_string()
    }
}

/// Save a plan response to disk.
///
/// Returns the slug used for the plan file.
pub fn save_plan(
    root: &Path,
    ctx: &PlannerContext,
    plan_response: &PlanToolResponse,
    model: &str,
    usage: Option<&Usage>,
    validation_warnings: Vec<String>,
) -> Result<String> {
    let dir = plans_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| error::io_err(&dir, e))?;

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let base_slug = slugify(&ctx.change_description);
    let slug = format!("{base_slug}-{timestamp}");

    let persisted = PersistedPlan {
        slug: slug.clone(),
        change_description: ctx.change_description.clone(),
        status: PlanStatus::Draft,
        created_at: Utc::now().to_rfc3339(),
        model: model.to_string(),
        usage: usage.cloned(),
        plans: plan_response.plans.clone(),
        context_summary: ContextSummary {
            scopes: ctx.relevant_scopes.clone(),
            diagram_count: ctx.selected_diagrams.len(),
            token_estimate: ctx.total_tokens_est,
        },
        validation_warnings,
    };

    let path = dir.join(format!("{slug}.json"));
    let json = serde_json::to_string_pretty(&persisted).map_err(|e| Error::Config(format!("Failed to serialize plan: {e}")))?;
    std::fs::write(&path, &json).map_err(|e| error::io_err(&path, e))?;

    info!(slug, path = %path.display(), "Saved plan");
    Ok(slug)
}

/// List all plans in the plans directory.
///
/// Returns plans sorted by creation date (newest first).
pub fn list_plans(root: &Path) -> Result<Vec<PersistedPlan>> {
    let dir = plans_dir(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| error::io_err(&dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| error::io_err(&dir, e))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            match load_plan_file(&path) {
                Ok(plan) => plans.push(plan),
                Err(e) => warn!(path = %path.display(), error = %e, "Skipping invalid plan file"),
            }
        }
    }

    // Sort newest first
    plans.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(plans)
}

/// Load a single plan from a file path.
fn load_plan_file(path: &Path) -> Result<PersistedPlan> {
    let content = std::fs::read_to_string(path).map_err(|e| error::io_err(path, e))?;
    serde_json::from_str(&content).map_err(|e| Error::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Load a plan by its slug.
pub fn load_plan(root: &Path, slug: &str) -> Result<PersistedPlan> {
    let path = plans_dir(root).join(format!("{slug}.json"));
    if !path.exists() {
        return Err(Error::Config(format!("Plan not found: {slug}")));
    }
    load_plan_file(&path)
}

/// Update the status of a plan.
pub fn update_plan_status(root: &Path, slug: &str, status: PlanStatus) -> Result<()> {
    let mut plan = load_plan(root, slug)?;
    plan.status = status.clone();

    let path = plans_dir(root).join(format!("{slug}.json"));
    let json = serde_json::to_string_pretty(&plan)
        .map_err(|e| Error::Config(format!("Failed to serialize plan: {e}")))?;
    std::fs::write(&path, &json).map_err(|e| error::io_err(&path, e))?;

    info!(slug, status = ?status, "Updated plan status");
    Ok(())
}

/// Approve a plan (shorthand for setting status to Approved).
pub fn approve_plan(root: &Path, slug: &str) -> Result<()> {
    update_plan_status(root, slug, PlanStatus::Approved)
}

/// Render a brief summary of a plan suitable for terminal display.
pub fn render_brief(plan: &PersistedPlan) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "Plan: {} [{}]",
        plan.slug,
        serde_json::to_string(&plan.status)
            .unwrap_or_else(|_| "unknown".to_string())
            .trim_matches('"')
    ));
    lines.push(format!("  Change: {}", plan.change_description));
    lines.push(format!("  Model: {}", plan.model));
    lines.push(format!("  Created: {}", plan.created_at));
    lines.push(format!(
        "  Context: {} scopes, {} diagrams, ~{} tokens",
        plan.context_summary.scopes.len(),
        plan.context_summary.diagram_count,
        plan.context_summary.token_estimate,
    ));

    if let Some(ref usage) = plan.usage {
        lines.push(format!(
            "  Usage: {} input + {} output tokens",
            usage.input_tokens, usage.output_tokens
        ));
    }

    lines.push(format!("  Variants: {}", plan.plans.len()));

    for (i, variant) in plan.plans.iter().enumerate() {
        lines.push(format!("    {}. {}", i + 1, variant.title));
        lines.push(format!("       {}", variant.summary));
        lines.push(format!(
            "       Files: {} changes, {} boundaries",
            variant.files_to_change.len(),
            variant.affected_boundaries.len()
        ));
        if let Some(ref u) = variant.uncertainty {
            lines.push(format!(
                "       Uncertainty: {}",
                serde_json::to_string(u)
                    .unwrap_or_else(|_| "?".to_string())
                    .trim_matches('"')
            ));
        }
        if !variant.risks.is_empty() {
            lines.push(format!("       Risks: {}", variant.risks.join("; ")));
        }
    }

    if !plan.validation_warnings.is_empty() {
        lines.push(format!(
            "  Warnings: {}",
            plan.validation_warnings.join("; ")
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Add a locations table"), "add-a-locations-table");
    }

    #[test]
    fn slugify_special_chars() {
        assert_eq!(
            slugify("Fix the auth/login & signup flow!"),
            "fix-the-auth-login-signup-flow"
        );
    }

    #[test]
    fn slugify_truncates_long_descriptions() {
        let long = "a".repeat(100);
        let slug = slugify(&long);
        assert!(slug.len() <= 50);
    }

    #[test]
    fn slugify_no_leading_trailing_hyphens() {
        let slug = slugify("  spaces around  ");
        assert!(!slug.starts_with('-'));
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn slugify_collapses_hyphens() {
        let slug = slugify("too   many   spaces");
        assert!(!slug.contains("--"));
    }

    #[test]
    fn save_and_load_plan() {
        let dir = tempfile::tempdir().unwrap();

        let ctx = PlannerContext {
            change_description: "Test change".to_string(),
            relevant_scopes: vec!["web".to_string()],
            system_diagram: String::new(),
            selected_diagrams: vec![],
            relevant_claude_mds: vec![],
            file_index_excerpt: "empty".to_string(),
            total_tokens_est: 100,
        };

        let plan_response = PlanToolResponse {
            plans: vec![Plan {
                title: "Test plan".to_string(),
                summary: "A test".to_string(),
                affected_boundaries: vec!["web".to_string()],
                context_pointers: vec![],
                edge_changes: vec![],
                diagram_changes: vec![],
                files_to_change: vec![FileChange {
                    path: "test.ts".to_string(),
                    action: FileAction::Create,
                    description: "Create file".to_string(),
                    depends_on: vec![],
                }],
                files_to_avoid: vec![],
                edge_violations: vec![],
                risks: vec![],
                uncertainty: Some(Uncertainty::Low),
            }],
        };

        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
        };

        let slug = save_plan(
            dir.path(),
            &ctx,
            &plan_response,
            "claude-opus-4-6",
            Some(&usage),
            vec![],
        )
        .unwrap();

        assert!(slug.starts_with("test-change-"));

        let loaded = load_plan(dir.path(), &slug).unwrap();
        assert_eq!(loaded.change_description, "Test change");
        assert_eq!(loaded.status, PlanStatus::Draft);
        assert_eq!(loaded.plans.len(), 1);
        assert_eq!(loaded.plans[0].title, "Test plan");
    }

    #[test]
    fn list_plans_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plans = list_plans(dir.path()).unwrap();
        assert!(plans.is_empty());
    }

    #[test]
    fn approve_plan_updates_status() {
        let dir = tempfile::tempdir().unwrap();

        let ctx = PlannerContext {
            change_description: "Approve test".to_string(),
            relevant_scopes: vec![],
            system_diagram: String::new(),
            selected_diagrams: vec![],
            relevant_claude_mds: vec![],
            file_index_excerpt: String::new(),
            total_tokens_est: 0,
        };

        let plan_response = PlanToolResponse {
            plans: vec![Plan {
                title: "Plan".to_string(),
                summary: "Summary".to_string(),
                affected_boundaries: vec![],
                context_pointers: vec![],
                edge_changes: vec![],
                diagram_changes: vec![],
                files_to_change: vec![],
                files_to_avoid: vec![],
                edge_violations: vec![],
                risks: vec![],
                uncertainty: None,
            }],
        };

        let slug = save_plan(dir.path(), &ctx, &plan_response, "test", None, vec![]).unwrap();

        approve_plan(dir.path(), &slug).unwrap();

        let loaded = load_plan(dir.path(), &slug).unwrap();
        assert_eq!(loaded.status, PlanStatus::Approved);
    }

    #[test]
    fn render_brief_output() {
        let plan = PersistedPlan {
            slug: "test-plan-20260219".to_string(),
            change_description: "Add locations".to_string(),
            status: PlanStatus::Draft,
            created_at: "2026-02-19T14:30:00Z".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: Some(Usage {
                input_tokens: 2000,
                output_tokens: 500,
            }),
            plans: vec![Plan {
                title: "Direct approach".to_string(),
                summary: "Create the table directly".to_string(),
                affected_boundaries: vec!["supabase".to_string()],
                context_pointers: vec![],
                edge_changes: vec![],
                diagram_changes: vec![],
                files_to_change: vec![
                    FileChange {
                        path: "migrations/001.sql".to_string(),
                        action: FileAction::Create,
                        description: "Create table".to_string(),
                        depends_on: vec![],
                    },
                    FileChange {
                        path: "types.ts".to_string(),
                        action: FileAction::Create,
                        description: "Add types".to_string(),
                        depends_on: vec![0],
                    },
                ],
                files_to_avoid: vec![],
                edge_violations: vec![],
                risks: vec!["May need backfill".to_string()],
                uncertainty: Some(Uncertainty::Low),
            }],
            context_summary: ContextSummary {
                scopes: vec!["supabase".to_string()],
                diagram_count: 2,
                token_estimate: 1500,
            },
            validation_warnings: vec![],
        };

        let brief = render_brief(&plan);
        assert!(brief.contains("test-plan-20260219"));
        assert!(brief.contains("draft"));
        assert!(brief.contains("Add locations"));
        assert!(brief.contains("Direct approach"));
        assert!(brief.contains("2 changes"));
        assert!(brief.contains("May need backfill"));
    }

    #[test]
    fn load_plan_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_plan(dir.path(), "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn list_plans_sorted_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let plans_path = dir.path().join(".claude").join("plans");
        std::fs::create_dir_all(&plans_path).unwrap();

        // Create two plans with different timestamps
        let plan1 = PersistedPlan {
            slug: "older".to_string(),
            change_description: "Old change".to_string(),
            status: PlanStatus::Draft,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            model: "test".to_string(),
            usage: None,
            plans: vec![],
            context_summary: ContextSummary {
                scopes: vec![],
                diagram_count: 0,
                token_estimate: 0,
            },
            validation_warnings: vec![],
        };

        let plan2 = PersistedPlan {
            slug: "newer".to_string(),
            change_description: "New change".to_string(),
            status: PlanStatus::Draft,
            created_at: "2026-02-01T00:00:00Z".to_string(),
            model: "test".to_string(),
            usage: None,
            plans: vec![],
            context_summary: ContextSummary {
                scopes: vec![],
                diagram_count: 0,
                token_estimate: 0,
            },
            validation_warnings: vec![],
        };

        std::fs::write(
            plans_path.join("older.json"),
            serde_json::to_string_pretty(&plan1).unwrap(),
        )
        .unwrap();
        std::fs::write(
            plans_path.join("newer.json"),
            serde_json::to_string_pretty(&plan2).unwrap(),
        )
        .unwrap();

        let plans = list_plans(dir.path()).unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].slug, "newer");
        assert_eq!(plans[1].slug, "older");
    }
}
