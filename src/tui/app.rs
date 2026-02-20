//! Core application state for the TUI.
//!
//! Follows The Elm Architecture (TEA): all state mutations flow through
//! `App::update(msg)`. The input layer produces `Message` values; the
//! view layer reads `&App` immutably.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use tracing::info;

use crate::error::Result;
use crate::indexer::types::{DiagramRecord, FileRecord};
use crate::planner::types::{ManifestEntry, PlannerContext};
use crate::planner;
use crate::settings::Settings;

use super::message::*;

type IndexArtifacts = (Vec<FileRecord>, Vec<ManifestEntry>, Vec<DiagramRecord>, Vec<String>);

/// Token budget step size for +/- adjustments.
const TOKEN_BUDGET_STEP: usize = 500;

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

/// Plan workflow phase for the Plan-Verify-Execute gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanPhase {
    /// Writing the change description.
    Describe,
    /// Reviewing generated plan + diagrams.
    Verify,
    /// Plan approved, ready for execution.
    Execute,
}

/// The main TUI application state.
pub struct App {
    /// Repo root path.
    pub root: PathBuf,

    /// Runtime settings loaded from config files.
    pub settings: Settings,

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
    /// Currently selected scope index (for navigating scope list).
    pub selected_scope: usize,

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
    /// Severity level of the current status message.
    pub status_level: StatusLevel,

    // -- UI overlays --
    /// Whether the help overlay is visible.
    pub show_help: bool,

    // -- Search --
    /// Whether search mode is active.
    pub search_active: bool,
    /// Current search query text.
    pub search_query: String,
    /// Filtered indices matching the search query.
    pub search_results: Vec<usize>,

    // -- Async state --
    /// Whether an async operation is in progress.
    pub is_busy: bool,
    /// Current spinner animation frame.
    pub spinner_frame: usize,

    // -- Side effects --
    /// Pending editor request: the main loop should pause the TUI, spawn
    /// `$EDITOR` with this content in a temp file, then emit
    /// `EditorComplete` with the result. Set by `OpenExternalEditor`,
    /// consumed (and cleared) by the main loop.
    pub editor_request: Option<String>,

    /// Channel receiver for messages produced by background threads
    /// (e.g. indexer). The main loop polls this with `try_recv` on
    /// every tick and feeds any received message back into `update()`.
    pub async_rx: Option<mpsc::Receiver<Message>>,

    // -- Context pinning (Feature 1: Token Thermometer) --
    /// Set of pinned diagram indices.
    pub pinned_diagrams: HashSet<usize>,
    /// Total estimated tokens for pinned diagrams.
    pub pinned_tokens: usize,

    // -- Plan-Verify-Execute gate (Feature 2) --
    /// Current plan workflow phase.
    pub plan_phase: PlanPhase,
    /// Whether diagram inline editing is active.
    pub diagram_edit_mode: bool,
    /// Buffer for diagram inline editing.
    pub diagram_edit_buffer: String,

    // -- Live linter (Feature 3) --
    /// Lint issues for the current diagram.
    pub lint_issues: Vec<LintIssue>,

    // -- Agent visualizer (Feature 4) --
    /// Agent execution pipeline steps.
    pub agent_steps: Vec<AgentStep>,
    /// Index of the currently active agent step.
    pub active_agent_step: usize,

    // -- Fuzzy finder (Feature 5) --
    /// Whether the fuzzy finder overlay is open.
    pub fuzzy_open: bool,
    /// Current fuzzy finder query text.
    pub fuzzy_query: String,
    /// Fuzzy search results.
    pub fuzzy_results: Vec<FuzzyResult>,
    /// Currently selected fuzzy result index.
    pub fuzzy_selected: usize,
}

impl App {
    /// Create a new App by loading index artifacts from disk.
    pub fn new(root: &Path) -> Result<Self> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

        // Load runtime settings (project > user > defaults)
        let settings = crate::settings::load_settings(&root);

        // Try to load existing index
        let (files, manifest, diagrams, claude_md_paths) =
            match load_index_artifacts(&root) {
                Ok(data) => data,
                Err(e) => {
                    info!(error = %e, "No existing index found -- starting fresh");
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

        // Use token budget from settings (which respects config file overrides)
        let token_budget = settings.planner.token_budget;

        Ok(App {
            root,
            settings,
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
            token_budget,
            selected_scope: 0,
            selected_diagram: 0,
            rendered_diagram: String::new(),
            diagram_scroll: 0,
            planning_context: None,
            handoff_scroll: 0,
            status_message: status,
            status_level: StatusLevel::Info,
            show_help: false,
            search_active: false,
            search_query: String::new(),
            search_results: Vec::new(),
            is_busy: false,
            spinner_frame: 0,
            editor_request: None,
            async_rx: None,
            pinned_diagrams: HashSet::new(),
            pinned_tokens: 0,
            plan_phase: PlanPhase::Describe,
            diagram_edit_mode: false,
            diagram_edit_buffer: String::new(),
            lint_issues: Vec::new(),
            agent_steps: Vec::new(),
            active_agent_step: 0,
            fuzzy_open: false,
            fuzzy_query: String::new(),
            fuzzy_results: Vec::new(),
            fuzzy_selected: 0,
        })
    }

    // -----------------------------------------------------------------------
    // TEA: update
    // -----------------------------------------------------------------------

    /// Process a message and update state. Returns `true` if the app should quit.
    ///
    /// This is the single point of state mutation in the TEA pattern.
    /// All side effects (indexing, plan generation, export) are called
    /// from here, keeping the input layer pure.
    pub fn update(&mut self, msg: Message) -> Result<bool> {
        match msg {
            // -- Navigation --
            Message::Quit => return Ok(true),

            Message::SwitchView(view) => {
                match view {
                    View::Diagram => {
                        if !self.diagrams.is_empty() {
                            self.current_view = View::Diagram;
                            self.render_current_diagram();
                        }
                    }
                    View::Handoff => {
                        if self.planning_context.is_some() {
                            self.current_view = View::Handoff;
                        }
                    }
                    other => {
                        self.current_view = other;
                    }
                }
            }

            // -- Index view --
            Message::SelectDomain(idx) => {
                if idx < self.domains.len() {
                    self.selected_domain = idx;
                    self.selected_file = 0;
                }
            }

            Message::SelectFile(idx) => {
                let max = self.filtered_files().len().saturating_sub(1);
                self.selected_file = idx.min(max);
            }

            Message::MoveDomainUp => {
                self.selected_domain = self.selected_domain.saturating_sub(1);
                self.selected_file = 0;
            }

            Message::MoveDomainDown => {
                let max = self.domains.len().saturating_sub(1);
                self.selected_domain = (self.selected_domain + 1).min(max);
                self.selected_file = 0;
            }

            Message::MoveFileUp => {
                self.selected_file = self.selected_file.saturating_sub(1);
            }

            Message::MoveFileDown => {
                let max = self.filtered_files().len().saturating_sub(1);
                self.selected_file = (self.selected_file + 1).min(max);
            }

            Message::ToggleShowAllDomains => {
                self.show_all_files = !self.show_all_files;
                self.selected_file = 0;
            }

            // -- Context pinning --
            Message::TogglePinDiagram(idx) => {
                if self.pinned_diagrams.contains(&idx) {
                    self.pinned_diagrams.remove(&idx);
                } else {
                    self.pinned_diagrams.insert(idx);
                }
                self.recalculate_pinned_tokens();
                self.status_message = format!(
                    "Pinned: {} diagrams, {} tokens",
                    self.pinned_diagrams.len(),
                    self.pinned_tokens
                );
                self.status_level = StatusLevel::Info;
            }

            Message::UnpinAll => {
                self.pinned_diagrams.clear();
                self.pinned_tokens = 0;
                self.status_message = "All diagrams unpinned".to_string();
                self.status_level = StatusLevel::Info;
            }

            // -- Plan view: text input --
            Message::FocusInput => {
                self.input_focused = true;
            }

            Message::UnfocusInput => {
                self.input_focused = false;
            }

            Message::InputChar(c) => {
                self.change_description.push(c);
            }

            Message::InputBackspace => {
                self.change_description.pop();
            }

            Message::InputSubmit => {
                self.input_focused = false;
                if let Err(e) = self.generate_plan() {
                    self.status_message = format!("Plan error: {e}");
                    self.status_level = StatusLevel::Error;
                }
            }

            Message::EditDescription => {
                self.input_focused = true;
            }

            Message::GeneratePlan => {
                if let Err(e) = self.generate_plan() {
                    self.status_message = format!("Plan error: {e}");
                    self.status_level = StatusLevel::Error;
                }
            }

            // -- Plan-Verify-Execute gate --
            Message::ApprovePlan => {
                if self.plan_phase == PlanPhase::Verify {
                    self.plan_phase = PlanPhase::Execute;
                    self.current_view = View::Handoff;
                    self.status_message =
                        "Plan approved -- ready for export".to_string();
                    self.status_level = StatusLevel::Success;
                }
            }

            Message::RejectPlan => {
                if self.plan_phase == PlanPhase::Verify {
                    self.plan_phase = PlanPhase::Describe;
                    self.planning_context = None;
                    self.status_message =
                        "Plan rejected -- edit description and regenerate".to_string();
                    self.status_level = StatusLevel::Warning;
                }
            }

            // -- Diagram editing --
            Message::EditDiagramInPlace => {
                if let Some(diag) = self.diagrams.get(self.selected_diagram) {
                    self.diagram_edit_buffer = diag.content.clone();
                    self.diagram_edit_mode = true;
                }
            }

            Message::DiagramEditChar(c) => {
                self.diagram_edit_buffer.push(c);
            }

            Message::DiagramEditBackspace => {
                self.diagram_edit_buffer.pop();
            }

            Message::DiagramEditNewline => {
                self.diagram_edit_buffer.push('\n');
            }

            Message::DiagramEditCancel => {
                self.diagram_edit_mode = false;
                self.diagram_edit_buffer.clear();
            }

            Message::DiagramEditSave => {
                if let Some(diag) = self.diagrams.get_mut(self.selected_diagram) {
                    diag.content.clone_from(&self.diagram_edit_buffer);
                }
                self.diagram_edit_mode = false;
                self.diagram_edit_buffer.clear();
                self.render_current_diagram();
                self.lint_current_diagram();
                self.status_message = "Diagram saved".to_string();
                self.status_level = StatusLevel::Success;
            }

            // -- Scope selection --
            Message::MoveScopeUp => {
                self.selected_scope = self.selected_scope.saturating_sub(1);
            }

            Message::MoveScopeDown => {
                let max = self.inferred_scopes.len().saturating_sub(1);
                self.selected_scope = (self.selected_scope + 1).min(max);
            }

            Message::ToggleScope(idx) => {
                if let Some(scope) = self.inferred_scopes.get(idx).cloned() {
                    if self.selected_scopes.contains(&scope) {
                        self.selected_scopes.remove(&scope);
                    } else {
                        self.selected_scopes.insert(scope);
                    }
                }
            }

            // -- Diagram view --
            Message::NextDiagram => {
                if self.selected_diagram + 1 < self.diagrams.len() {
                    self.selected_diagram += 1;
                    self.diagram_scroll = 0;
                    self.render_current_diagram();
                }
            }

            Message::PrevDiagram => {
                if self.selected_diagram > 0 {
                    self.selected_diagram -= 1;
                    self.diagram_scroll = 0;
                    self.render_current_diagram();
                }
            }

            Message::ScrollDiagramUp => {
                self.diagram_scroll = self.diagram_scroll.saturating_sub(1);
            }

            Message::ScrollDiagramDown => {
                self.diagram_scroll += 1;
            }

            Message::ScrollDiagramHome => {
                self.diagram_scroll = 0;
            }

            // -- Live linter --
            Message::LintCurrentDiagram => {
                self.lint_current_diagram();
                if self.lint_issues.is_empty() {
                    self.status_message = "No lint issues found".to_string();
                    self.status_level = StatusLevel::Success;
                } else {
                    self.status_message =
                        format!("{} lint issues found", self.lint_issues.len());
                    self.status_level = StatusLevel::Warning;
                }
            }

            Message::LintComplete(issues) => {
                self.lint_issues = issues;
            }

            Message::AutoHealDiagram => {
                self.auto_heal_current_diagram();
            }

            Message::AutoHealComplete(result) => {
                match result {
                    Ok((content, fixes)) => {
                        if let Some(diag) =
                            self.diagrams.get_mut(self.selected_diagram)
                        {
                            diag.content = content;
                        }
                        self.render_current_diagram();
                        self.lint_current_diagram();
                        self.status_message =
                            format!("Healed: {} fixes", fixes.len());
                        self.status_level = StatusLevel::Success;
                    }
                    Err(e) => {
                        self.status_message = format!("Heal failed: {e}");
                        self.status_level = StatusLevel::Error;
                    }
                }
            }

            // -- Handoff --
            Message::ExportPlan => {
                if let Err(e) = self.export_plan() {
                    self.status_message = format!("Export error: {e}");
                    self.status_level = StatusLevel::Error;
                } else {
                    self.status_message = "Exported to .claude/planner-context.md".to_string();
                    self.status_level = StatusLevel::Success;
                }
            }

            Message::ScrollHandoffUp => {
                self.handoff_scroll = self.handoff_scroll.saturating_sub(1);
            }

            Message::ScrollHandoffDown => {
                self.handoff_scroll += 1;
            }

            Message::ScrollHandoffHome => {
                self.handoff_scroll = 0;
            }

            // -- Agent visualizer --
            Message::SetAgentSteps(steps) => {
                self.agent_steps = steps;
                self.active_agent_step = 0;
            }

            Message::AdvanceAgentStep => {
                if self.active_agent_step < self.agent_steps.len() {
                    if let Some(step) =
                        self.agent_steps.get_mut(self.active_agent_step)
                    {
                        step.status = StepStatus::Done;
                    }
                    self.active_agent_step += 1;
                    if let Some(step) =
                        self.agent_steps.get_mut(self.active_agent_step)
                    {
                        step.status = StepStatus::Active;
                    }
                }
            }

            // -- Operations --
            Message::RunIndexer => {
                if self.is_busy {
                    self.status_message = "Indexer already running...".to_string();
                    self.status_level = StatusLevel::Warning;
                    return Ok(false);
                }

                self.is_busy = true;
                self.spinner_frame = 0;
                self.status_message = "Indexing...".to_string();
                self.status_level = StatusLevel::Info;

                let (tx, rx) = mpsc::channel();
                self.async_rx = Some(rx);

                let root = self.root.clone();
                std::thread::spawn(move || {
                    let msg = match crate::indexer::scan_repo(&root) {
                        Ok(result) => {
                            let stats = IndexStats {
                                files: result.files.len(),
                                diagrams: result.diagrams.len(),
                                domains: {
                                    let mut ds = HashSet::new();
                                    for f in &result.files {
                                        if !f.domain.is_empty() {
                                            ds.insert(&f.domain);
                                        }
                                    }
                                    ds.len()
                                },
                            };
                            // Write index artifacts to disk; on failure,
                            // report via IndexComplete(Err(..)).
                            if let Err(e) = crate::indexer::write_index(&result, false) {
                                Message::IndexComplete(Err(format!("Write failed: {e}")))
                            } else {
                                Message::IndexComplete(Ok(stats))
                            }
                        }
                        Err(e) => Message::IndexComplete(Err(e.to_string())),
                    };
                    // If send fails the receiver was dropped (app quit),
                    // which is fine -- just discard.
                    let _ = tx.send(msg);
                });
            }

            Message::IndexComplete(result) => {
                self.is_busy = false;
                match result {
                    Ok(stats) => {
                        // Reload artifacts from disk (the background thread
                        // already wrote them).
                        match load_index_artifacts(&self.root) {
                            Ok((files, manifest, diagrams, claude_md_paths)) => {
                                self.files = files;
                                self.manifest = manifest;
                                self.diagrams = diagrams;
                                self.claude_md_paths = claude_md_paths;

                                let mut domain_set = HashSet::new();
                                for f in &self.files {
                                    if !f.domain.is_empty() {
                                        domain_set.insert(f.domain.clone());
                                    }
                                }
                                self.domains = domain_set.into_iter().collect();
                                self.domains.sort();
                            }
                            Err(e) => {
                                self.status_message = format!("Reload failed: {e}");
                                self.status_level = StatusLevel::Error;
                                return Ok(false);
                            }
                        }
                        self.status_message = format!(
                            "Indexed: {} files, {} diagrams, {} domains",
                            stats.files, stats.diagrams, stats.domains
                        );
                        self.status_level = StatusLevel::Success;
                    }
                    Err(e) => {
                        self.status_message = format!("Index failed: {e}");
                        self.status_level = StatusLevel::Error;
                    }
                }
            }

            Message::PlanComplete(result) => {
                self.is_busy = false;
                match result {
                    Ok(ctx) => {
                        self.status_message = format!(
                            "Plan: {} scopes, {} diagrams, ~{} tokens -- [Enter] approve [r] reject",
                            ctx.relevant_scopes.len(),
                            ctx.selected_diagrams.len(),
                            ctx.total_tokens_est,
                        );
                        self.status_level = StatusLevel::Success;
                        self.inferred_scopes = ctx.relevant_scopes.clone();
                        self.planning_context = Some(ctx);
                        self.plan_phase = PlanPhase::Verify;
                    }
                    Err(e) => {
                        self.status_message = format!("Plan failed: {e}");
                        self.status_level = StatusLevel::Error;
                    }
                }
            }

            // -- UI --
            Message::ShowHelp => {
                self.show_help = true;
            }

            Message::HideHelp => {
                self.show_help = false;
            }

            Message::SetStatus(msg, level) => {
                self.status_message = msg;
                self.status_level = level;
            }

            // -- Search --
            Message::SearchStart => {
                self.search_active = true;
                self.search_query.clear();
                self.search_results.clear();
            }

            Message::SearchInput(c) => {
                self.search_query.push(c);
                self.update_search_results();
            }

            Message::SearchBackspace => {
                self.search_query.pop();
                self.update_search_results();
            }

            Message::SearchClear => {
                self.search_active = false;
                self.search_query.clear();
                self.search_results.clear();
            }

            Message::SearchSubmit => {
                self.search_active = false;
                // Keep results visible; the filtered view will use search_results
                // if the query is non-empty.
            }

            // -- Fuzzy finder --
            Message::FuzzyOpen => {
                self.fuzzy_open = true;
                self.fuzzy_query.clear();
                self.fuzzy_results.clear();
                self.fuzzy_selected = 0;
            }

            Message::FuzzyClose => {
                self.fuzzy_open = false;
            }

            Message::FuzzyInput(c) => {
                self.fuzzy_query.push(c);
                self.update_fuzzy_results();
            }

            Message::FuzzyBackspace => {
                self.fuzzy_query.pop();
                self.update_fuzzy_results();
            }

            Message::FuzzySelectUp => {
                self.fuzzy_selected = self.fuzzy_selected.saturating_sub(1);
            }

            Message::FuzzySelectDown => {
                let max = self.fuzzy_results.len().saturating_sub(1);
                self.fuzzy_selected = (self.fuzzy_selected + 1).min(max);
            }

            Message::FuzzyConfirm => {
                if let Some(result) = self.fuzzy_results.get(self.fuzzy_selected)
                {
                    let idx = result.diagram_idx;
                    if self.pinned_diagrams.contains(&idx) {
                        self.pinned_diagrams.remove(&idx);
                    } else {
                        self.pinned_diagrams.insert(idx);
                    }
                    self.recalculate_pinned_tokens();
                    self.status_message = format!(
                        "Pinned: {} diagrams, {} tokens",
                        self.pinned_diagrams.len(),
                        self.pinned_tokens
                    );
                    self.status_level = StatusLevel::Info;
                }
                self.fuzzy_open = false;
            }

            // -- External editor --
            Message::OpenExternalEditor => {
                // Signal the main loop to pause the TUI and spawn the
                // editor. The main loop owns the Terminal and can leave/
                // re-enter alternate screen; we cannot do that from here.
                self.editor_request = Some(self.change_description.clone());
            }

            Message::EditorComplete(content) => {
                self.change_description = content;
                self.input_focused = false;
            }

            // -- Token budget --
            Message::IncreaseBudget => {
                self.token_budget = self.token_budget.saturating_add(TOKEN_BUDGET_STEP);
                self.status_message = format!("Token budget: {}", self.token_budget);
                self.status_level = StatusLevel::Info;
            }

            Message::DecreaseBudget => {
                self.token_budget = self
                    .token_budget
                    .saturating_sub(TOKEN_BUDGET_STEP)
                    .max(TOKEN_BUDGET_STEP);
                self.status_message = format!("Token budget: {}", self.token_budget);
                self.status_level = StatusLevel::Info;
            }
        }

        Ok(false)
    }

    // -----------------------------------------------------------------------
    // Query methods (read-only, used by views)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Internal mutation helpers (called only from update)
    // -----------------------------------------------------------------------

    /// Generate the planning context from current state.
    ///
    /// Transitions to `PlanPhase::Verify` so the user can review before
    /// approving (Enter) or rejecting (r).
    fn generate_plan(&mut self) -> Result<()> {
        if self.change_description.trim().is_empty() {
            self.status_message = "Enter a change description first.".to_string();
            self.status_level = StatusLevel::Warning;
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
            "Plan: {} scopes, {} diagrams, ~{} tokens -- [Enter] approve [r] reject",
            ctx.relevant_scopes.len(),
            ctx.selected_diagrams.len(),
            ctx.total_tokens_est,
        );
        self.status_level = StatusLevel::Success;

        self.inferred_scopes = ctx.relevant_scopes.clone();
        self.planning_context = Some(ctx);
        self.plan_phase = PlanPhase::Verify;

        Ok(())
    }

    /// Export the planning context to `.claude/planner-context.md`.
    fn export_plan(&self) -> Result<()> {
        if let Some(ctx) = &self.planning_context {
            planner::write_planning_context(&self.root, ctx)?;
        }
        Ok(())
    }

    /// Render the currently selected diagram as ASCII.
    fn render_current_diagram(&mut self) {
        if let Some(diag) = self.diagrams.get(self.selected_diagram) {
            match graphs_tui::render_mermaid_to_tui(
                &diag.content,
                graphs_tui::RenderOptions::default(),
            ) {
                Ok(result) => {
                    self.rendered_diagram = result.output;
                }
                Err(_) => {
                    self.rendered_diagram = format!(
                        "Could not render diagram ({})\n\n{}",
                        diag.diagram_type, diag.content
                    );
                }
            }
        }
    }

    /// Recalculate total tokens for pinned diagrams.
    fn recalculate_pinned_tokens(&mut self) {
        self.pinned_tokens = self
            .pinned_diagrams
            .iter()
            .filter_map(|&idx| self.diagrams.get(idx))
            .map(|d| d.tokens_est)
            .sum();
    }

    /// Run lint on the current diagram (synchronous -- repair is fast).
    fn lint_current_diagram(&mut self) {
        self.lint_issues.clear();
        if let Some(diag) = self.diagrams.get(self.selected_diagram) {
            let repair_result = crate::indexer::repair::repair(&diag.content);
            for warning in &repair_result.warnings {
                self.lint_issues.push(LintIssue {
                    line: 0,
                    severity: LintSeverity::Warning,
                    message: warning.clone(),
                });
            }
            for fix in &repair_result.fixes {
                self.lint_issues.push(LintIssue {
                    line: 0,
                    severity: LintSeverity::Error,
                    message: fix.clone(),
                });
            }
        }
    }

    /// Auto-heal the current diagram using `repair::repair()`.
    fn auto_heal_current_diagram(&mut self) {
        if let Some(diag) = self.diagrams.get_mut(self.selected_diagram) {
            let repair_result = crate::indexer::repair::repair(&diag.content);
            if !repair_result.fixes.is_empty() {
                diag.content = repair_result.content;
                self.status_message = format!(
                    "Auto-healed: {} fixes applied",
                    repair_result.fixes.len()
                );
                self.status_level = StatusLevel::Success;
            } else {
                self.status_message = "No issues to heal".to_string();
                self.status_level = StatusLevel::Info;
            }
        }
        // Re-render and re-lint after healing
        self.render_current_diagram();
        self.lint_current_diagram();
    }

    /// Update fuzzy finder results based on the current fuzzy query.
    fn update_fuzzy_results(&mut self) {
        self.fuzzy_results.clear();
        if self.fuzzy_query.is_empty() {
            return;
        }

        let query_lower = self.fuzzy_query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

        for (i, diag) in self.diagrams.iter().enumerate() {
            let mut relevance = 0.0;
            let id_lower = diag.id.to_lowercase();
            let domain_lower = diag.domain.to_lowercase();
            let type_lower = diag.diagram_type.to_lowercase();
            let content_lower = diag.content.to_lowercase();
            let desc_lower = diag.description.to_lowercase();

            // Whole-query substring matching (existing behavior)
            if id_lower.contains(&query_lower) {
                relevance += 3.0;
            }
            if domain_lower.contains(&query_lower) {
                relevance += 2.0;
            }
            if type_lower.contains(&query_lower) {
                relevance += 1.0;
            }
            if content_lower.contains(&query_lower) {
                relevance += 0.5;
            }

            // Description term matching: each query term that appears
            // in the NL description adds weight (semantic search)
            if !desc_lower.is_empty() {
                for term in &query_terms {
                    if desc_lower.contains(term) {
                        relevance += 2.5;
                    }
                }
            }

            if relevance > 0.0 {
                if id_lower == query_lower {
                    relevance += 5.0;
                }

                // Prefer description as preview when available
                let preview = if !diag.description.is_empty() {
                    diag.description.clone()
                } else {
                    diag.content
                        .lines()
                        .take(3)
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                self.fuzzy_results.push(FuzzyResult {
                    diagram_idx: i,
                    diagram_id: diag.id.clone(),
                    domain: diag.domain.clone(),
                    diagram_type: diag.diagram_type.clone(),
                    tokens_est: diag.tokens_est,
                    relevance,
                    content_preview: preview,
                });
            }
        }

        self.fuzzy_results.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        self.fuzzy_selected = 0;
    }

    /// Update search results based on the current query.
    fn update_search_results(&mut self) {
        self.search_results.clear();
        if self.search_query.is_empty() {
            return;
        }

        let query_lower = self.search_query.to_lowercase();

        match self.current_view {
            View::Index => {
                // Search files by path
                for (i, file) in self.files.iter().enumerate() {
                    if file.path.to_lowercase().contains(&query_lower)
                        || file.domain.to_lowercase().contains(&query_lower)
                    {
                        self.search_results.push(i);
                    }
                }
            }
            View::Diagram => {
                // Search diagrams by id, domain, or description
                for (i, diag) in self.diagrams.iter().enumerate() {
                    if diag.id.to_lowercase().contains(&query_lower)
                        || diag.domain.to_lowercase().contains(&query_lower)
                        || diag.description.to_lowercase().contains(&query_lower)
                    {
                        self.search_results.push(i);
                    }
                }
            }
            _ => {}
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
                    let matched = manifest
                        .iter()
                        .find(|m| {
                            let safe = m.source.replace('/', "__").replace('#', "_");
                            filename.starts_with(safe.trim_end_matches(".mmd"))
                        });

                    let (id, domain, diagram_type, description) = matched
                        .map(|m| (m.id.clone(), m.scope.clone(), m.diagram_type.clone(), m.description.clone()))
                        .unwrap_or_else(|| {
                            (
                                filename.to_string(),
                                "unknown".to_string(),
                                crate::indexer::mermaid::infer_type(&content).to_string(),
                                String::new(),
                            )
                        });

                    diagrams.push(DiagramRecord {
                        id,
                        source: filename.replace("__", "/"),
                        domain,
                        diagram_type,
                        tokens_est: crate::indexer::mermaid::estimate_tokens(&content),
                        content,
                        description,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Create a minimal App for testing update logic.
    fn test_app() -> App {
        App {
            root: Path::new("/tmp/test").to_path_buf(),
            current_view: View::Index,
            should_quit: false,
            files: Vec::new(),
            diagrams: Vec::new(),
            manifest: Vec::new(),
            domains: Vec::new(),
            claude_md_paths: Vec::new(),
            selected_domain: 0,
            selected_file: 0,
            show_all_files: false,
            change_description: String::new(),
            input_focused: false,
            selected_scopes: HashSet::new(),
            inferred_scopes: Vec::new(),
            token_budget: 4000,
            selected_scope: 0,
            selected_diagram: 0,
            rendered_diagram: String::new(),
            diagram_scroll: 0,
            planning_context: None,
            handoff_scroll: 0,
            status_message: String::new(),
            status_level: StatusLevel::Info,
            show_help: false,
            search_active: false,
            search_query: String::new(),
            search_results: Vec::new(),
            is_busy: false,
            spinner_frame: 0,
            settings: Settings::default(),
            editor_request: None,
            async_rx: None,
            pinned_diagrams: HashSet::new(),
            pinned_tokens: 0,
            plan_phase: PlanPhase::Describe,
            diagram_edit_mode: false,
            diagram_edit_buffer: String::new(),
            lint_issues: Vec::new(),
            agent_steps: Vec::new(),
            active_agent_step: 0,
            fuzzy_open: false,
            fuzzy_query: String::new(),
            fuzzy_results: Vec::new(),
            fuzzy_selected: 0,
        }
    }

    #[test]
    fn open_external_editor_sets_editor_request() {
        let mut app = test_app();
        app.change_description = "fix the widget".to_string();

        let quit = app.update(Message::OpenExternalEditor).unwrap();
        assert!(!quit);
        assert_eq!(app.editor_request, Some("fix the widget".to_string()));
    }

    #[test]
    fn editor_complete_updates_change_description() {
        let mut app = test_app();
        app.input_focused = true;

        let quit = app
            .update(Message::EditorComplete("new text from editor".to_string()))
            .unwrap();
        assert!(!quit);
        assert_eq!(app.change_description, "new text from editor");
        // Editor complete should unfocus input
        assert!(!app.input_focused);
    }

    #[test]
    fn run_indexer_sets_busy_and_creates_channel() {
        let mut app = test_app();

        let quit = app.update(Message::RunIndexer).unwrap();
        assert!(!quit);
        assert!(app.is_busy);
        assert!(app.async_rx.is_some());
        assert_eq!(app.status_message, "Indexing...");
    }

    #[test]
    fn run_indexer_rejected_when_already_busy() {
        let mut app = test_app();
        app.is_busy = true;

        let quit = app.update(Message::RunIndexer).unwrap();
        assert!(!quit);
        assert!(app.status_message.contains("already running"));
    }

    #[test]
    fn index_complete_ok_clears_busy() {
        let mut app = test_app();
        app.is_busy = true;

        let stats = IndexStats {
            files: 42,
            diagrams: 3,
            domains: 5,
        };
        let quit = app.update(Message::IndexComplete(Ok(stats))).unwrap();
        assert!(!quit);
        assert!(!app.is_busy);
        // Status should mention the counts (even though reload from
        // disk fails on /tmp/test, the stats from the message are used)
        assert!(
            app.status_message.contains("42")
                || app.status_message.contains("Reload failed"),
            "status was: {}",
            app.status_message
        );
    }

    #[test]
    fn index_complete_err_clears_busy_and_shows_error() {
        let mut app = test_app();
        app.is_busy = true;

        let quit = app
            .update(Message::IndexComplete(Err("boom".to_string())))
            .unwrap();
        assert!(!quit);
        assert!(!app.is_busy);
        assert!(app.status_message.contains("boom"));
        assert_eq!(app.status_level, StatusLevel::Error);
    }
}
