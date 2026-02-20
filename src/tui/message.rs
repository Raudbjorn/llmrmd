//! Message types for the TUI's Elm Architecture (TEA) pattern.
//!
//! Every user action and async completion is expressed as a `Message`.
//! The `App::update()` method is the sole consumer of these messages,
//! ensuring all state mutations flow through a single function.

use crate::planner::types::PlannerContext;

use super::app::View;

// -----------------------------------------------------------------------
// Supporting types used as message payloads
// -----------------------------------------------------------------------

/// Severity for lint issues found in diagrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    Warning,
    Error,
}

/// A single lint issue in a diagram.
#[derive(Debug, Clone)]
pub struct LintIssue {
    pub line: usize,
    pub severity: LintSeverity,
    pub message: String,
}

/// A fuzzy search result for the structural fuzzy finder.
#[derive(Debug, Clone)]
pub struct FuzzyResult {
    pub diagram_idx: usize,
    pub diagram_id: String,
    pub domain: String,
    pub diagram_type: String,
    pub tokens_est: usize,
    pub relevance: f64,
    pub content_preview: String,
}

// -----------------------------------------------------------------------
// Message enum
// -----------------------------------------------------------------------

/// All possible messages/events in the TUI.
///
/// Key design constraint: `input.rs` maps `KeyEvent` -> `Option<Message>`,
/// then `app.update(msg)` performs the actual state mutation. No state
/// mutation happens in the input layer.
#[derive(Debug, Clone)]
pub enum Message {
    // -- Navigation --
    /// Switch to a specific view tab.
    SwitchView(View),
    /// Quit the application.
    Quit,

    // -- Index view --
    /// Select a domain by index.
    SelectDomain(usize),
    /// Select a file by index.
    SelectFile(usize),
    /// Move domain selection up.
    MoveDomainUp,
    /// Move domain selection down.
    MoveDomainDown,
    /// Move file selection up.
    MoveFileUp,
    /// Move file selection down.
    MoveFileDown,
    /// Toggle between showing all files vs. filtered by domain.
    ToggleShowAllDomains,

    // -- Context pinning (Feature 1: Token Thermometer) --
    /// Toggle pinning a diagram to the context budget.
    TogglePinDiagram(usize),
    /// Remove all pinned diagrams.
    UnpinAll,

    // -- Plan view --
    /// Focus the text input for editing the change description.
    FocusInput,
    /// Unfocus the text input (Esc while editing).
    UnfocusInput,
    /// Append a character to the change description.
    InputChar(char),
    /// Delete the last character from the change description.
    InputBackspace,
    /// Submit the text input (Enter while editing).
    InputSubmit,
    /// Focus edit mode on the description field (from plan view).
    EditDescription,
    /// Trigger plan generation from the current description.
    GeneratePlan,

    // -- Plan-Verify-Execute gate (Feature 2) --
    /// Approve the plan and advance to Execute phase.
    ApprovePlan,
    /// Reject the plan and return to Describe phase.
    RejectPlan,

    // -- Diagram editing (Feature 2 & 3) --
    /// Enter diagram inline editing mode.
    EditDiagramInPlace,
    /// Append a character in diagram edit mode.
    DiagramEditChar(char),
    /// Delete last character in diagram edit mode.
    DiagramEditBackspace,
    /// Insert newline in diagram edit mode.
    DiagramEditNewline,
    /// Cancel diagram editing (discard changes).
    DiagramEditCancel,
    /// Save diagram edits and re-render.
    DiagramEditSave,

    // -- Scope selection --
    /// Move scope selection cursor up.
    MoveScopeUp,
    /// Move scope selection cursor down.
    MoveScopeDown,
    /// Toggle a scope's inclusion by index.
    ToggleScope(usize),

    // -- Diagram view --
    /// Navigate to the next diagram.
    NextDiagram,
    /// Navigate to the previous diagram.
    PrevDiagram,
    /// Scroll the diagram content up.
    ScrollDiagramUp,
    /// Scroll the diagram content down.
    ScrollDiagramDown,
    /// Reset diagram scroll to top.
    ScrollDiagramHome,

    // -- Live linter (Feature 3) --
    /// Run lint on the current diagram.
    LintCurrentDiagram,
    /// Lint completed with results.
    LintComplete(Vec<LintIssue>),
    /// Auto-heal the current diagram via repair.
    AutoHealDiagram,
    /// Auto-heal completed: Ok((new_content, fixes)) or Err(reason).
    AutoHealComplete(Result<(String, Vec<String>), String>),

    // -- Handoff --
    /// Export the planning context to disk.
    ExportPlan,
    /// Scroll the handoff prompt preview up.
    ScrollHandoffUp,
    /// Scroll the handoff prompt preview down.
    ScrollHandoffDown,
    /// Reset handoff scroll to top.
    ScrollHandoffHome,

    // -- Agent DAG executor --
    /// Create a GraphExecutor with default_plan_graph and start the pipeline.
    StartAgentPipeline,
    /// Approve the current WaitingForGate node.
    ApproveGate,
    /// Skip the current node.
    SkipAgentStep,
    /// A step completed successfully.
    AgentStepComplete { node_id: String, output: Option<String> },
    /// A step failed.
    AgentStepFailed { node_id: String, error: String },

    // -- Operations --
    /// Trigger the filesystem indexer.
    RunIndexer,
    /// Indexer completed (future async use).
    IndexComplete(Result<IndexStats, String>),
    /// Plan generation completed (future async use).
    PlanComplete(Result<PlannerContext, String>),

    // -- UI --
    /// Show the keybinding help overlay.
    ShowHelp,
    /// Dismiss the keybinding help overlay.
    HideHelp,
    /// Set a status bar message with severity level.
    SetStatus(String, StatusLevel),

    // -- Search --
    /// Enter search mode.
    SearchStart,
    /// Append a character to the search query.
    SearchInput(char),
    /// Delete the last character from the search query.
    SearchBackspace,
    /// Clear search and exit search mode.
    SearchClear,
    /// Execute the search and exit search mode.
    SearchSubmit,

    // -- Fuzzy finder (Feature 5) --
    /// Open the fuzzy finder overlay.
    FuzzyOpen,
    /// Close the fuzzy finder overlay.
    FuzzyClose,
    /// Append a character to the fuzzy query.
    FuzzyInput(char),
    /// Delete last character from the fuzzy query.
    FuzzyBackspace,
    /// Move fuzzy selection up.
    FuzzySelectUp,
    /// Move fuzzy selection down.
    FuzzySelectDown,
    /// Confirm fuzzy selection (pin to budget).
    FuzzyConfirm,

    // -- External editor --
    /// Launch $EDITOR for the change description.
    OpenExternalEditor,
    /// Editor returned with new content.
    EditorComplete(String),

    // -- Token budget --
    /// Increase the token budget by one step.
    IncreaseBudget,
    /// Decrease the token budget by one step.
    DecreaseBudget,
}

/// Statistics from an indexing run (for `Message::IndexComplete`).
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files: usize,
    pub diagrams: usize,
    pub domains: usize,
    pub boundaries: usize,
    pub edges: usize,
}

/// Severity level for status bar messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Success,
    Warning,
    Error,
}
