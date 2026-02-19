//! Core application state for the TUI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::info;

use crate::error::Result;
use crate::indexer::types::{DiagramRecord, FileRecord};
use crate::planner::types::{ManifestEntry, PlannerContext};
use crate::planner;

type IndexArtifacts = (Vec<FileRecord>, Vec<ManifestEntry>, Vec<DiagramRecord>, Vec<String>);

/// Which panel/view is currently focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Browse domains and files.
    Index,
    /// Write change description + select scopes.
    Plan,
    /// View a mermaid diagram.
    Diagram,
    /// Review generated planning context.
    Handoff,
}

/// The main TUI application state.
pub struct App {
    /// Repo root path.
    pub root: PathBuf,

    /// Current focused view.
    pub current_view: View,

    /// Whether we should quit.
    pub should_quit: bool,

    // -- Index data --
    /// All indexed files.
    pub files: Vec<FileRecord>,
    /// All extracted diagrams.
    pub diagrams: Vec<DiagramRecord>,
    /// Manifest entries.
    pub manifest: Vec<ManifestEntry>,
    /// All unique domains found.
    pub domains: Vec<String>,
    /// CLAUDE.md paths.
    pub claude_md_paths: Vec<String>,

    // -- Index view state --
    /// Currently selected domain index.
    pub selected_domain: usize,
    /// Currently selected file index (within filtered list).
    pub selected_file: usize,
    /// Show files for all domains or just selected.
    pub show_all_files: bool,

    // -- Plan view state --
    /// Change description text being edited.
    pub change_description: String,
    /// Whether the text input is focused.
    pub input_focused: bool,
    /// Manually selected/deselected scopes.
    pub selected_scopes: HashSet<String>,
    /// Auto-inferred scopes (before manual override).
    pub inferred_scopes: Vec<String>,
    /// Token budget.
    pub token_budget: usize,

    // -- Diagram view state --
    /// Currently selected diagram index.
    pub selected_diagram: usize,
    /// Rendered ASCII content of current diagram.
    pub rendered_diagram: String,
    /// Scroll offset for diagram view.
    pub diagram_scroll: u16,

    // -- Handoff state --
    /// Generated planning context (if any).
    pub planning_context: Option<PlannerContext>,
    /// Scroll offset for handoff view.
    pub handoff_scroll: u16,

    // -- Status bar --
    pub status_message: String,
}

impl App {
    /// Create a new App by loading index artifacts from disk.
    pub fn new(root: &Path) -> Result<Self> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

        // Try to load existing index
        let (files, manifest, diagrams, claude_md_paths) =
            match load_index_artifacts(&root) {
                Ok(data) => data,
                Err(e) => {
                    info!(error = %e, "No existing index found — starting fresh");
                    (Vec::new(), Vec::new(), Vec::new(), Vec::new())
                }
            };

        // Extract unique domains
        let mut domain_set: HashSet<String> = HashSet::new();
        for f in &files {
            if !f.domain.is_empty() {
                domain_set.insert(f.domain.clone());
            }
        }
        let mut domains: Vec<String> = domain_set.into_iter().collect();
        domains.sort();

        let status = if files.is_empty() {
            "No index found. Press 'i' to run indexer, or 'q' to quit.".to_string()
        } else {
            format!(
                "{} files, {} diagrams, {} domains",
                files.len(),
                diagrams.len(),
                domains.len()
            )
        };

        Ok(App {
            root,
            current_view: View::Index,
            should_quit: false,
            files,
            diagrams,
            manifest,
            domains,
            claude_md_paths,
            selected_domain: 0,
            selected_file: 0,
            show_all_files: false,
            change_description: String::new(),
            input_focused: false,
            selected_scopes: HashSet::new(),
            inferred_scopes: Vec::new(),
            token_budget: crate::config::DEFAULT_TOKEN_BUDGET,
            selected_diagram: 0,
            rendered_diagram: String::new(),
            diagram_scroll: 0,
            planning_context: None,
            handoff_scroll: 0,
            status_message: status,
        })
    }

    /// Get files filtered by the currently selected domain.
    pub fn filtered_files(&self) -> Vec<&FileRecord> {
        if self.show_all_files || self.domains.is_empty() {
            self.files.iter().collect()
        } else if let Some(domain) = self.domains.get(self.selected_domain) {
            self.files.iter().filter(|f| &f.domain == domain).collect()
        } else {
            self.files.iter().collect()
        }
    }

    /// Run the indexer and reload artifacts.
    pub fn run_indexer(&mut self) -> Result<()> {
        self.status_message = "Indexing...".to_string();

        let result = crate::indexer::scan_repo(&self.root)?;
        crate::indexer::write_index(&result, false)?;

        // Reload
        let (files, manifest, diagrams, claude_md_paths) =
            load_index_artifacts(&self.root)?;

        self.files = files;
        self.manifest = manifest;
        self.diagrams = diagrams;
        self.claude_md_paths = claude_md_paths;

        let mut domain_set: HashSet<String> = HashSet::new();
        for f in &self.files {
            if !f.domain.is_empty() {
                domain_set.insert(f.domain.clone());
            }
        }
        self.domains = domain_set.into_iter().collect();
        self.domains.sort();

        self.status_message = format!(
            "Indexed: {} files, {} diagrams, {} domains",
            self.files.len(),
            self.diagrams.len(),
            self.domains.len()
        );

        Ok(())
    }

    /// Generate the planning context from current state.
    pub fn generate_plan(&mut self) -> Result<()> {
        if self.change_description.trim().is_empty() {
            self.status_message = "Enter a change description first.".to_string();
            return Ok(());
        }

        let ctx = planner::select_context(
            &self.root,
            &self.change_description,
            &self.manifest,
            &self.files,
            self.token_budget,
        );

        self.status_message = format!(
            "Plan: {} scopes, {} diagrams, ~{} tokens",
            ctx.relevant_scopes.len(),
            ctx.selected_diagrams.len(),
            ctx.total_tokens_est,
        );

        self.inferred_scopes = ctx.relevant_scopes.clone();
        self.planning_context = Some(ctx);
        self.current_view = View::Handoff;

        Ok(())
    }

    /// Export the planning context to `.claude/planner-context.md`.
    pub fn export_plan(&self) -> Result<()> {
        if let Some(ctx) = &self.planning_context {
            planner::write_planning_context(&self.root, ctx)?;
        }
        Ok(())
    }

    /// Render the currently selected diagram as ASCII.
    pub fn render_current_diagram(&mut self) {
        if let Some(diag) = self.diagrams.get(self.selected_diagram) {
            // Try graphs-tui for ASCII rendering
            match graphs_tui::render_mermaid_to_tui(&diag.content, graphs_tui::RenderOptions::default())
            {
                Ok(result) => {
                    self.rendered_diagram = result.output;
                }
                Err(_) => {
                    // Fallback: show raw mermaid source
                    self.rendered_diagram = format!(
                        "⚠ Could not render diagram ({})\n\n{}",
                        diag.diagram_type, diag.content
                    );
                }
            }
        }
    }
}

/// Load all index artifacts from `.claude/`.
fn load_index_artifacts(
    root: &Path,
) -> Result<IndexArtifacts> {
    let files = planner::load_file_index(root)?;
    let manifest = planner::load_manifest(root)?;

    // Load diagrams from manifest + extracted files
    let out = crate::config::output_dir(root);
    let diag_dir = out.join("diagrams").join("extracted");

    let mut diagrams: Vec<DiagramRecord> = Vec::new();
    if diag_dir.exists() {
        for entry in std::fs::read_dir(&diag_dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("mmd") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let filename = path
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");

                    // Try to find matching manifest entry
                    let (id, domain, diagram_type) = manifest
                        .iter()
                        .find(|m| {
                            let safe = m.source.replace('/', "__").replace('#', "_");
                            filename.starts_with(safe.trim_end_matches(".mmd"))
                        })
                        .map(|m| (m.id.clone(), m.scope.clone(), m.diagram_type.clone()))
                        .unwrap_or_else(|| {
                            (
                                filename.to_string(),
                                "unknown".to_string(),
                                crate::indexer::mermaid::infer_type(&content).to_string(),
                            )
                        });

                    diagrams.push(DiagramRecord {
                        id,
                        source: filename.replace("__", "/"),
                        domain,
                        diagram_type,
                        tokens_est: crate::indexer::mermaid::estimate_tokens(&content),
                        content,
                    });
                }
            }
        }
    }

    let claude_md_paths: Vec<String> = files
        .iter()
        .filter(|f| f.file_type == "claude_md")
        .map(|f| f.path.clone())
        .collect();

    Ok((files, manifest, diagrams, claude_md_paths))
}
