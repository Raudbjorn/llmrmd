//! Planner: Assembles minimal context from indexer artifacts for LLM planning.

pub mod plans;
pub mod scope;
pub mod types;

use std::path::Path;

use tracing::{info, warn};

use crate::config::{output_dir, CHARS_PER_TOKEN, DEFAULT_TOKEN_BUDGET};
use crate::error::{self, Error, Result};
use crate::indexer::toon;
use types::*;

// ---------------------------------------------------------------------------
// Loading artifacts
// ---------------------------------------------------------------------------

/// Load the diagram manifest from `.claude/manifest.json`.
pub fn load_manifest(root: &Path) -> Result<Vec<ManifestEntry>> {
    let path = output_dir(root).join("manifest.json");
    if !path.exists() {
        return Err(Error::NoIndex(path));
    }

    let data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| error::io_err(&path, e))?,
    )
    .map_err(|e| Error::Json {
        path: path.clone(),
        source: e,
    })?;

    let entries: Vec<ManifestEntry> = data
        .get("diagrams")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some(ManifestEntry {
                        id: d.get("id")?.as_str()?.to_string(),
                        source: d.get("source")?.as_str()?.to_string(),
                        scope: d.get("domain").or_else(|| d.get("scope"))?.as_str()?.to_string(),
                        diagram_type: d.get("type")?.as_str()?.to_string(),
                        tokens_est: d.get("tokens_est")?.as_u64()? as usize,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    info!(count = entries.len(), "Loaded manifest");
    Ok(entries)
}

/// Load the file index from `.claude/file-index.json`.
pub fn load_file_index(root: &Path) -> Result<Vec<crate::indexer::types::FileRecord>> {
    let path = output_dir(root).join("file-index.json");
    if !path.exists() {
        return Err(Error::NoIndex(path));
    }

    let data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| error::io_err(&path, e))?,
    )
    .map_err(|e| Error::Json {
        path: path.clone(),
        source: e,
    })?;

    let files: Vec<crate::indexer::types::FileRecord> = data
        .get("files")
        .and_then(|f| serde_json::from_value(f.clone()).ok())
        .unwrap_or_default();

    info!(count = files.len(), "Loaded file index");
    Ok(files)
}

// ---------------------------------------------------------------------------
// Context selection
// ---------------------------------------------------------------------------

fn read_diagram_content(root: &Path, source: &str) -> String {
    // Try extracted location first
    let safe_name = format!(
        "{}.mmd",
        source
            .replace('/', "__")
            .replace('#', "_")
            .trim_end_matches(".mmd")
    );
    let extracted = output_dir(root)
        .join("diagrams")
        .join("extracted")
        .join(&safe_name);

    if let Ok(content) = std::fs::read_to_string(&extracted) {
        return content;
    }

    // Try original source (strip #diag-N suffix)
    let original = source.split('#').next().unwrap_or(source);
    let abs_path = root.join(original);
    if let Ok(content) = std::fs::read_to_string(&abs_path) {
        return content;
    }

    warn!(source, "Could not read diagram content");
    String::new()
}

fn read_claude_md(root: &Path, claude_md_path: &str) -> String {
    let abs_path = root.join(claude_md_path);
    std::fs::read_to_string(&abs_path).unwrap_or_default()
}

fn filter_file_index_toon(
    files: &[crate::indexer::types::FileRecord],
    scopes: &[String],
) -> String {
    let relevant: Vec<&crate::indexer::types::FileRecord> = files
        .iter()
        .filter(|f| scopes.contains(&f.domain) || scopes.contains(&"root".to_string()))
        .collect();

    if relevant.is_empty() {
        return "# No files in selected scopes.".to_string();
    }

    let rows: Vec<Vec<&str>> = relevant
        .iter()
        .map(|f| vec![f.path.as_str(), f.file_type.as_str(), f.domain.as_str()])
        .collect();

    let comment = format!(
        "Filtered file index ({} files in scopes: {})",
        relevant.len(),
        scopes.join(", ")
    );

    toon::render_tabular("files", &["path", "type", "domain"], &rows, Some(&comment))
}

/// Select the minimal context needed for planning a change.
pub fn select_context(
    root: &Path,
    change_description: &str,
    manifest: &[ManifestEntry],
    files: &[crate::indexer::types::FileRecord],
    token_budget: usize,
) -> PlannerContext {
    select_context_with_scopes(root, change_description, manifest, files, token_budget, None)
}

/// Select context with optional explicit scope override.
pub fn select_context_with_scopes(
    root: &Path,
    change_description: &str,
    manifest: &[ManifestEntry],
    files: &[crate::indexer::types::FileRecord],
    token_budget: usize,
    explicit_scopes: Option<&[String]>,
) -> PlannerContext {
    let budget = if token_budget == 0 {
        DEFAULT_TOKEN_BUDGET
    } else {
        token_budget
    };

    let scopes = match explicit_scopes {
        Some(s) if !s.is_empty() => {
            info!(scopes = ?s, "Using explicit scopes");
            s.to_vec()
        }
        _ => scope::infer_scopes(change_description, manifest, files),
    };

    // 1. System diagram (root scope)
    let mut system_diagram = String::new();
    for entry in manifest {
        if entry.scope == "root" {
            let content = read_diagram_content(root, &entry.source);
            if !content.is_empty() {
                system_diagram = content;
                break;
            }
        }
    }

    let mut tokens_used = system_diagram.len() / CHARS_PER_TOKEN;

    // 2. Scope-relevant diagrams (sorted smallest first, within budget)
    let mut selected: Vec<(ManifestEntry, String)> = Vec::new();
    let mut sorted_manifest: Vec<&ManifestEntry> = manifest.iter().collect();
    sorted_manifest.sort_by_key(|e| e.tokens_est);

    for entry in sorted_manifest {
        if entry.scope != "root" && scopes.contains(&entry.scope) {
            if tokens_used + entry.tokens_est > budget {
                info!(
                    source = entry.source,
                    tokens = entry.tokens_est,
                    used = tokens_used,
                    budget,
                    "Skipping diagram — would exceed budget"
                );
                continue;
            }
            let content = read_diagram_content(root, &entry.source);
            if !content.is_empty() {
                tokens_used += entry.tokens_est;
                selected.push((entry.clone(), content));
            }
        }
    }

    // 3. Relevant CLAUDE.md files
    let claude_md_paths: Vec<String> = files
        .iter()
        .filter(|f| f.file_type == "claude_md")
        .map(|f| f.path.clone())
        .collect();

    let mut relevant_claude_mds: Vec<(String, String)> = Vec::new();
    for cpath in &claude_md_paths {
        let path_parts: Vec<&str> = cpath.split('/').collect();
        let matches_scope = scopes.iter().any(|scope| {
            path_parts.contains(&scope.as_str()) || scope == "root"
        });

        if matches_scope {
            let content = read_claude_md(root, cpath);
            if !content.is_empty() {
                tokens_used += content.len() / CHARS_PER_TOKEN;
                relevant_claude_mds.push((cpath.clone(), content));
            }
        }
    }

    // 4. Filtered file index
    let file_excerpt = filter_file_index_toon(files, &scopes);
    tokens_used += file_excerpt.len() / CHARS_PER_TOKEN;

    info!(
        scopes = scopes.len(),
        diagrams = selected.len(),
        claude_mds = relevant_claude_mds.len(),
        tokens = tokens_used,
        "Planning context assembled"
    );

    PlannerContext {
        change_description: change_description.to_string(),
        relevant_scopes: scopes,
        system_diagram,
        selected_diagrams: selected,
        relevant_claude_mds,
        file_index_excerpt: file_excerpt,
        total_tokens_est: tokens_used,
    }
}

// ---------------------------------------------------------------------------
// Prompt rendering
// ---------------------------------------------------------------------------

/// The system prompt injected into the planning context.
pub const PLANNER_SYSTEM_PROMPT: &str = include_str!("planner_prompt.txt");

/// Render the complete planning prompt ready for an LLM.
pub fn render_planning_prompt(ctx: &PlannerContext) -> String {
    let mut sections = Vec::new();

    sections.push(PLANNER_SYSTEM_PROMPT.to_string());
    sections.push("---\n".to_string());

    if !ctx.system_diagram.is_empty() {
        sections.push("## System Architecture (current state)\n".to_string());
        sections.push(format!("```mermaid\n{}\n```\n", ctx.system_diagram));
    }

    if !ctx.selected_diagrams.is_empty() {
        sections.push("## Component Diagrams (current state)\n".to_string());
        for (entry, content) in &ctx.selected_diagrams {
            sections.push(format!(
                "### {} ({}, ~{} tokens)\n",
                entry.source, entry.diagram_type, entry.tokens_est
            ));
            sections.push(format!("```mermaid\n{content}\n```\n"));
        }
    }

    if !ctx.relevant_claude_mds.is_empty() {
        sections.push("## Project Conventions\n".to_string());
        for (path, content) in &ctx.relevant_claude_mds {
            sections.push(format!("### {path}\n"));
            sections.push(format!("{content}\n"));
        }
    }

    sections.push("## Relevant Files\n".to_string());
    sections.push(format!("```toon\n{}\n```\n", ctx.file_index_excerpt));

    sections.push("---\n".to_string());
    sections.push("## Change Request\n".to_string());
    sections.push(format!("{}\n", ctx.change_description));
    sections.push("\n---\n".to_string());
    sections.push(format!(
        "*Planning context: {} scopes, {} diagrams, ~{} tokens*\n",
        ctx.relevant_scopes.len(),
        ctx.selected_diagrams.len(),
        ctx.total_tokens_est,
    ));

    sections.join("\n")
}

/// Write the planning context to `.claude/planner-context.md`.
pub fn write_planning_context(root: &Path, ctx: &PlannerContext) -> Result<()> {
    let out = output_dir(root);
    std::fs::create_dir_all(&out).map_err(|e| error::io_err(&out, e))?;

    let prompt = render_planning_prompt(ctx);
    let output_path = out.join("planner-context.md");
    std::fs::write(&output_path, &prompt).map_err(|e| error::io_err(&output_path, e))?;

    info!(
        path = %output_path.display(),
        bytes = prompt.len(),
        tokens = ctx.total_tokens_est,
        "Wrote planning context"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::types::FileRecord;

    fn sample_files() -> Vec<FileRecord> {
        vec![
            FileRecord {
                path: "apps/web/src/index.ts".to_string(),
                file_type: "module".to_string(),
                domain: "web".to_string(),
                subdomain: "core".to_string(),
                claude_md: "CLAUDE.md".to_string(),
            },
            FileRecord {
                path: "apps/admin/src/index.ts".to_string(),
                file_type: "module".to_string(),
                domain: "admin".to_string(),
                subdomain: "core".to_string(),
                claude_md: String::new(),
            },
            FileRecord {
                path: "CLAUDE.md".to_string(),
                file_type: "claude_md".to_string(),
                domain: "root".to_string(),
                subdomain: "core".to_string(),
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
            },
            ManifestEntry {
                id: "web-flow".to_string(),
                source: "apps/web/flow.mmd".to_string(),
                scope: "web".to_string(),
                diagram_type: "flowchart".to_string(),
                tokens_est: 80,
            },
        ]
    }

    #[test]
    fn select_context_returns_relevant_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let files = sample_files();
        let manifest = sample_manifest();

        let ctx = select_context(dir.path(), "Update the web frontend", &manifest, &files, 4000);
        assert!(ctx.relevant_scopes.contains(&"web".to_string()));
    }

    #[test]
    fn select_context_with_explicit_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let files = sample_files();
        let manifest = sample_manifest();
        let explicit = vec!["admin".to_string()];

        let ctx = select_context_with_scopes(
            dir.path(),
            "Some change",
            &manifest,
            &files,
            4000,
            Some(&explicit),
        );
        assert_eq!(ctx.relevant_scopes, vec!["admin".to_string()]);
    }

    #[test]
    fn select_context_uses_default_budget() {
        let dir = tempfile::tempdir().unwrap();
        let files = sample_files();
        let manifest = sample_manifest();

        // Budget 0 should use DEFAULT_TOKEN_BUDGET
        let ctx = select_context(dir.path(), "test", &manifest, &files, 0);
        assert!(ctx.total_tokens_est <= DEFAULT_TOKEN_BUDGET + 1000); // some slack for overhead
    }

    #[test]
    fn render_planning_prompt_contains_sections() {
        let ctx = PlannerContext {
            change_description: "Add a locations table".to_string(),
            relevant_scopes: vec!["supabase".to_string()],
            system_diagram: "flowchart LR\n  A-->B".to_string(),
            selected_diagrams: vec![(
                ManifestEntry {
                    id: "web-flow".to_string(),
                    source: "web/flow.mmd".to_string(),
                    scope: "web".to_string(),
                    diagram_type: "flowchart".to_string(),
                    tokens_est: 50,
                },
                "flowchart LR\n  C-->D".to_string(),
            )],
            relevant_claude_mds: vec![("CLAUDE.md".to_string(), "# Conventions".to_string())],
            file_index_excerpt: "files[1]{path,type,domain}:\ntest.ts,module,web".to_string(),
            total_tokens_est: 200,
        };

        let prompt = render_planning_prompt(&ctx);
        assert!(prompt.contains("System Architecture"));
        assert!(prompt.contains("flowchart LR"));
        assert!(prompt.contains("Component Diagrams"));
        assert!(prompt.contains("Project Conventions"));
        assert!(prompt.contains("Relevant Files"));
        assert!(prompt.contains("Change Request"));
        assert!(prompt.contains("Add a locations table"));
    }

    #[test]
    fn render_planning_prompt_empty_context() {
        let ctx = PlannerContext {
            change_description: "Simple change".to_string(),
            relevant_scopes: vec!["root".to_string()],
            system_diagram: String::new(),
            selected_diagrams: Vec::new(),
            relevant_claude_mds: Vec::new(),
            file_index_excerpt: "# No files in selected scopes.".to_string(),
            total_tokens_est: 10,
        };

        let prompt = render_planning_prompt(&ctx);
        assert!(prompt.contains("Change Request"));
        assert!(prompt.contains("Simple change"));
        assert!(!prompt.contains("System Architecture"));
        assert!(!prompt.contains("Component Diagrams"));
    }

    #[test]
    fn write_planning_context_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = PlannerContext {
            change_description: "Test change".to_string(),
            relevant_scopes: vec!["root".to_string()],
            system_diagram: String::new(),
            selected_diagrams: Vec::new(),
            relevant_claude_mds: Vec::new(),
            file_index_excerpt: "empty".to_string(),
            total_tokens_est: 5,
        };

        write_planning_context(dir.path(), &ctx).unwrap();

        let output = dir.path().join(".claude").join("planner-context.md");
        assert!(output.exists());
        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.contains("Test change"));
    }

    #[test]
    fn load_manifest_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_manifest(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_file_index_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_file_index(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_manifest_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let manifest = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "diagrams": [
                {
                    "id": "test-diag",
                    "source": "test.mmd",
                    "domain": "root",
                    "type": "flowchart",
                    "tokens_est": 50
                }
            ]
        });
        std::fs::write(
            claude_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let entries = load_manifest(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "test-diag");
        assert_eq!(entries[0].scope, "root");
    }

    #[test]
    fn load_file_index_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let index = serde_json::json!({
            "generated_at": "2026-01-01T00:00:00Z",
            "root": "test",
            "claude_md_paths": [],
            "files": [
                {
                    "path": "src/main.rs",
                    "file_type": "module",
                    "domain": "root",
                    "subdomain": "core",
                    "claude_md": ""
                }
            ]
        });
        std::fs::write(
            claude_dir.join("file-index.json"),
            serde_json::to_string_pretty(&index).unwrap(),
        )
        .unwrap();

        let files = load_file_index(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
    }
}
