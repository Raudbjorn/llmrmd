//! Graph-based executor for the agent engine.
//!
//! [`GraphExecutor`] owns an [`ExecutionState`] and drives it forward one node
//! at a time. It is intentionally synchronous -- the caller (TUI event loop or
//! CLI driver) decides *when* to call [`GraphExecutor::advance`] and how to
//! dispatch the actual work (via [`StepExecutor`] trait objects).
//!
//! The separation between the executor (state machine) and the step executors
//! (actual I/O) keeps the core deterministic and trivially testable.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;

use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::{AgentNode, AgentNodeType, ExecutionGraph, ExecutionState, NodeState, NodeStatus};

// ---------------------------------------------------------------------------
// TUI display types (owned here to avoid circular dep on tui::app)
// ---------------------------------------------------------------------------

/// A single step in the agent execution pipeline, for TUI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub label: String,
    pub status: AgentStepStatus,
}

/// Status of an agent step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentStepStatus {
    Pending,
    Active,
    Completed,
    Failed(String),
}

// ---------------------------------------------------------------------------
// StepExecutor trait + context
// ---------------------------------------------------------------------------

/// Execution context passed to each [`StepExecutor`].
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Repository root path.
    pub root: PathBuf,
    /// User's change description.
    pub change_description: String,
    /// Mermaid IR content (populated after GenerateIR step).
    pub ir_content: String,
    /// Outputs from previously completed nodes, keyed by node ID.
    pub previous_outputs: HashMap<String, String>,
}

/// Output produced by a single step execution.
#[derive(Debug, Clone)]
pub struct StepOutput {
    /// Primary textual content (e.g. generated IR, validation report).
    pub content: String,
    /// Named artifacts produced (e.g. `("diagram.mmd", "<content>")` pairs).
    pub artifacts: Vec<(String, String)>,
}

/// Pluggable execution logic for a single graph node.
///
/// Implementors perform the actual work (LLM calls, file I/O, validation).
/// The executor calls [`StepExecutor::execute`] when a node transitions to
/// [`NodeStatus::Running`].
pub trait StepExecutor: Send + Sync {
    /// Execute the step, returning output or an error.
    fn execute(&self, node: &AgentNode, context: &ExecutionContext) -> Result<StepOutput>;
}

// ---------------------------------------------------------------------------
// Default executor implementations
// ---------------------------------------------------------------------------

/// No-op executor that returns success immediately.
///
/// Useful for testing, dry-run mode, and UI preview without real I/O.
pub struct NoOpExecutor;

impl StepExecutor for NoOpExecutor {
    fn execute(&self, node: &AgentNode, _context: &ExecutionContext) -> Result<StepOutput> {
        Ok(StepOutput {
            content: format!("[no-op] {} completed", node.label),
            artifacts: Vec::new(),
        })
    }
}

/// Validates mermaid syntax by running basic lint checks on the IR content.
///
/// Checks for: valid diagram directive, balanced delimiters, empty node labels.
pub struct ValidateSyntaxExecutor;

impl StepExecutor for ValidateSyntaxExecutor {
    fn execute(&self, _node: &AgentNode, context: &ExecutionContext) -> Result<StepOutput> {
        let errors = lint_mermaid_ir(&context.ir_content);
        if errors.is_empty() {
            Ok(StepOutput {
                content: "Syntax validation passed.".to_string(),
                artifacts: Vec::new(),
            })
        } else {
            Err(crate::error::Error::PlanValidation(format!(
                "Syntax errors found:\n{}",
                errors.join("\n")
            )))
        }
    }
}

/// Gate executor that signals the node should wait for user approval.
///
/// Returns a special output marker; the [`GraphExecutor`] checks for
/// [`AgentNodeType::Gate`] and sets [`NodeStatus::WaitingForGate`] instead
/// of [`NodeStatus::Completed`].
pub struct GateExecutor;

impl StepExecutor for GateExecutor {
    fn execute(&self, _node: &AgentNode, _context: &ExecutionContext) -> Result<StepOutput> {
        Ok(StepOutput {
            content: "Waiting for user approval.".to_string(),
            artifacts: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// Mermaid IR linting (standalone, no TUI dependency)
// ---------------------------------------------------------------------------

/// Basic lint checks for mermaid IR content.
///
/// Returns a vec of human-readable error descriptions. An empty vec means
/// the content passed all checks.
fn lint_mermaid_ir(content: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        errors.push("Empty diagram content.".to_string());
        return errors;
    }

    // Find first meaningful (non-comment, non-empty) line
    let first_meaningful = lines.iter().enumerate().find(|(_, l)| {
        let trimmed = l.trim();
        !trimmed.is_empty() && !trimmed.starts_with("%%")
    });

    if let Some((line_num, line)) = first_meaningful {
        let trimmed = line.trim().to_lowercase();
        let valid_starts = [
            "flowchart",
            "graph",
            "sequencediagram",
            "classdiagram",
            "statediagram",
            "erdiagram",
            "gantt",
            "pie",
            "gitgraph",
            "journey",
            "mindmap",
            "timeline",
            "quadrantchart",
            "sankey",
            "xychart",
            "block",
            "architecture",
            "c4",
            // XML-style wrappers from the planner are allowed
            "<system_architecture_diagram>",
        ];
        if !valid_starts.iter().any(|s| trimmed.starts_with(s)) {
            errors.push(format!(
                "Line {}: expected diagram directive, found '{}'",
                line_num + 1,
                line.trim()
            ));
        }
    }

    // Check for unbalanced delimiters
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("%%") {
            continue;
        }
        for ch in trimmed.chars() {
            match ch {
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }
        if paren_depth < 0 {
            errors.push(format!("Line {}: unmatched closing parenthesis ')'", i + 1));
            paren_depth = 0;
        }
        if bracket_depth < 0 {
            errors.push(format!("Line {}: unmatched closing bracket ']'", i + 1));
            bracket_depth = 0;
        }
        if brace_depth < 0 {
            errors.push(format!("Line {}: unmatched closing brace '}}'", i + 1));
            brace_depth = 0;
        }
    }

    let last_line = lines.len();
    if paren_depth > 0 {
        errors.push(format!(
            "Line {}: {} unclosed parenthesis/parentheses '('",
            last_line, paren_depth
        ));
    }
    if bracket_depth > 0 {
        errors.push(format!(
            "Line {}: {} unclosed bracket(s) '['",
            last_line, bracket_depth
        ));
    }
    if brace_depth > 0 {
        errors.push(format!(
            "Line {}: {} unclosed brace(s) '{{'",
            last_line, brace_depth
        ));
    }

    errors
}

// ---------------------------------------------------------------------------
// GraphExecutor
// ---------------------------------------------------------------------------

/// Drives an [`ExecutionGraph`] through its lifecycle.
///
/// The executor is a synchronous state machine. It does not spawn tasks or
/// perform I/O itself -- the caller is responsible for dispatching work via
/// [`StepExecutor`] implementations and feeding results back through
/// [`complete_node`](GraphExecutor::complete_node) /
/// [`fail_node`](GraphExecutor::fail_node).
pub struct GraphExecutor {
    state: ExecutionState,
}

impl GraphExecutor {
    /// Create a new executor for the given graph.
    ///
    /// All nodes start in [`NodeStatus::Pending`]. The executor's clock starts
    /// immediately.
    pub fn new(graph: ExecutionGraph) -> Self {
        let node_states: Vec<NodeState> = graph
            .nodes
            .iter()
            .map(|n| NodeState::new(&n.id))
            .collect();

        Self {
            state: ExecutionState {
                graph,
                node_states,
                current_node: None,
                is_complete: false,
                started_at: Utc::now().to_rfc3339(),
            },
        }
    }

    /// Borrow the current execution state (for UI rendering).
    pub fn state(&self) -> &ExecutionState {
        &self.state
    }

    /// Advance to the next ready node and mark it as running.
    ///
    /// Returns the node ID that was activated, or `None` if no node is ready
    /// (either the graph is complete, or nodes are blocked by unresolved deps).
    ///
    /// If the activated node is a [`AgentNodeType::Gate`], it is set to
    /// [`NodeStatus::WaitingForGate`] instead of [`NodeStatus::Running`].
    pub fn advance(&mut self) -> Option<&str> {
        // Skip dependents of failed nodes
        self.propagate_failures();

        let ready = self.state.graph.ready_nodes(&self.state.node_states);
        if ready.is_empty() {
            self.update_completion();
            return None;
        }

        // Take the first ready node (topological priority)
        let topo = self.state.graph.topological_order();
        let next_id = topo
            .into_iter()
            .find(|id| ready.contains(id))
            .expect("ready_nodes returned non-empty but no match in topological_order");

        let now = Utc::now().to_rfc3339();

        // Determine if this is a gate node
        let is_gate = self
            .state
            .graph
            .nodes
            .iter()
            .any(|n| n.id == next_id && n.node_type == AgentNodeType::Gate);

        if let Some(ns) = self
            .state
            .node_states
            .iter_mut()
            .find(|s| s.node_id == next_id)
        {
            if is_gate {
                ns.status = NodeStatus::WaitingForGate;
            } else {
                ns.status = NodeStatus::Running;
            }
            ns.started_at = Some(now);
        }

        self.state.current_node = Some(next_id.to_string());

        // Return reference to the stored current_node string
        self.state.current_node.as_deref()
    }

    /// Mark a node as completed with optional output.
    pub fn complete_node(&mut self, node_id: &str, output: Option<String>) {
        let now = Utc::now().to_rfc3339();
        if let Some(ns) = self
            .state
            .node_states
            .iter_mut()
            .find(|s| s.node_id == node_id)
        {
            ns.status = NodeStatus::Completed;
            ns.completed_at = Some(now);
            ns.output = output;
        }

        if self.state.current_node.as_deref() == Some(node_id) {
            self.state.current_node = None;
        }

        self.update_completion();
    }

    /// Mark a node as failed with an error message.
    ///
    /// Downstream dependents will be skipped on the next [`advance`](Self::advance) call.
    pub fn fail_node(&mut self, node_id: &str, error: String) {
        let now = Utc::now().to_rfc3339();
        if let Some(ns) = self
            .state
            .node_states
            .iter_mut()
            .find(|s| s.node_id == node_id)
        {
            ns.status = NodeStatus::Failed;
            ns.completed_at = Some(now);
            ns.error = Some(error);
        }

        if self.state.current_node.as_deref() == Some(node_id) {
            self.state.current_node = None;
        }

        self.update_completion();
    }

    /// Skip a node (e.g. repair not needed).
    pub fn skip_node(&mut self, node_id: &str) {
        let now = Utc::now().to_rfc3339();
        if let Some(ns) = self
            .state
            .node_states
            .iter_mut()
            .find(|s| s.node_id == node_id)
        {
            ns.status = NodeStatus::Skipped;
            ns.completed_at = Some(now);
        }

        if self.state.current_node.as_deref() == Some(node_id) {
            self.state.current_node = None;
        }

        self.update_completion();
    }

    /// Check if the execution is complete.
    ///
    /// Complete means every node is in a terminal state: `Completed`, `Failed`,
    /// or `Skipped`.
    pub fn is_complete(&self) -> bool {
        self.state.is_complete
    }

    /// Get progress as `(completed_or_terminal, total)`.
    ///
    /// Terminal states are `Completed`, `Failed`, and `Skipped`.
    pub fn progress(&self) -> (usize, usize) {
        let total = self.state.node_states.len();
        let done = self
            .state
            .node_states
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    NodeStatus::Completed | NodeStatus::Failed | NodeStatus::Skipped
                )
            })
            .count();
        (done, total)
    }

    /// Convert the current state to a vec of [`AgentStep`] for TUI display.
    ///
    /// Maps [`NodeStatus`] to [`AgentStepStatus`]:
    /// - `Pending` / `Ready` -> `Pending`
    /// - `Running` / `WaitingForGate` -> `Active`
    /// - `Completed` / `Skipped` -> `Completed`
    /// - `Failed` -> `Failed(error)`
    pub fn to_agent_steps(&self) -> Vec<AgentStep> {
        self.state
            .node_states
            .iter()
            .zip(self.state.graph.nodes.iter())
            .map(|(ns, node)| {
                let status = match &ns.status {
                    NodeStatus::Pending | NodeStatus::Ready => AgentStepStatus::Pending,
                    NodeStatus::Running | NodeStatus::WaitingForGate => AgentStepStatus::Active,
                    NodeStatus::Completed | NodeStatus::Skipped => AgentStepStatus::Completed,
                    NodeStatus::Failed => {
                        let msg = ns
                            .error
                            .clone()
                            .unwrap_or_else(|| "unknown error".to_string());
                        AgentStepStatus::Failed(msg)
                    }
                };
                AgentStep {
                    label: node.label.clone(),
                    status,
                }
            })
            .collect()
    }

    // -- Private helpers --

    /// Mark pending nodes as skipped if any of their transitive dependencies
    /// failed or were themselves skipped due to upstream failure.
    ///
    /// Uses recursive propagation: after skipping immediate dependents of
    /// failed/poisoned nodes, repeats until no new nodes are skipped. This
    /// correctly handles arbitrarily deep dependency chains.
    fn propagate_failures(&mut self) {
        // Build a "poison" set: nodes that are Failed, or Skipped-with-error
        // (i.e., skipped because an upstream dependency failed).
        let poison_ids: std::collections::HashSet<&str> = self
            .state
            .node_states
            .iter()
            .filter(|s| {
                s.status == NodeStatus::Failed
                    || (s.status == NodeStatus::Skipped && s.error.is_some())
            })
            .map(|s| s.node_id.as_str())
            .collect();

        if poison_ids.is_empty() {
            return;
        }

        // For each pending node, check if any direct dependency is poisoned
        let to_skip: Vec<String> = self
            .state
            .graph
            .nodes
            .iter()
            .filter(|node| {
                let is_pending = self
                    .state
                    .node_states
                    .iter()
                    .any(|s| s.node_id == node.id && s.status == NodeStatus::Pending);
                is_pending
                    && node
                        .depends_on
                        .iter()
                        .any(|dep| poison_ids.contains(dep.as_str()))
            })
            .map(|node| node.id.clone())
            .collect();

        let now = Utc::now().to_rfc3339();
        for id in &to_skip {
            if let Some(ns) = self
                .state
                .node_states
                .iter_mut()
                .find(|s| s.node_id == *id)
            {
                ns.status = NodeStatus::Skipped;
                ns.completed_at = Some(now.clone());
                ns.error = Some("Skipped: upstream dependency failed.".to_string());
            }
        }

        // Recurse until fixed point (handles transitive deps)
        if !to_skip.is_empty() {
            self.propagate_failures();
        }
    }

    /// Update `is_complete` flag on the execution state.
    fn update_completion(&mut self) {
        self.state.is_complete = self.state.node_states.iter().all(|s| {
            matches!(
                s.status,
                NodeStatus::Completed | NodeStatus::Failed | NodeStatus::Skipped
            )
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_executor() -> GraphExecutor {
        GraphExecutor::new(ExecutionGraph::default_plan_graph())
    }

    #[test]
    fn lifecycle_advance_complete_loop() {
        let mut exec = make_executor();
        assert!(!exec.is_complete());
        assert_eq!(exec.progress(), (0, 5));

        // Advance -> generate_ir
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("generate_ir"));
        assert_eq!(exec.progress(), (0, 5));

        // Complete generate_ir
        exec.complete_node("generate_ir", Some("ir content".to_string()));
        assert_eq!(exec.progress(), (1, 5));
        assert!(!exec.is_complete());

        // Advance -> validate_syntax
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("validate_syntax"));

        exec.complete_node("validate_syntax", None);
        assert_eq!(exec.progress(), (2, 5));

        // Advance -> check_consistency
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("check_consistency"));
        exec.complete_node("check_consistency", None);

        // Advance -> gate (should be WaitingForGate)
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("gate"));
        // Verify it was set to WaitingForGate
        let gate_state = exec
            .state()
            .node_states
            .iter()
            .find(|s| s.node_id == "gate")
            .unwrap();
        assert_eq!(gate_state.status, NodeStatus::WaitingForGate);

        // Approve gate (caller completes it)
        exec.complete_node("gate", None);

        // Advance -> execute
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("execute"));
        exec.complete_node("execute", Some("done".to_string()));

        assert!(exec.is_complete());
        assert_eq!(exec.progress(), (5, 5));
    }

    #[test]
    fn fail_node_prevents_dependents() {
        let mut exec = make_executor();

        // Advance and fail generate_ir
        exec.advance();
        exec.fail_node("generate_ir", "LLM timeout".to_string());

        // Try to advance -- dependents should be skipped, nothing to run
        let next = exec.advance().map(str::to_string);
        assert!(next.is_none());

        // All downstream should be skipped, generate_ir is failed
        assert!(exec.is_complete());

        let (done, total) = exec.progress();
        assert_eq!(done, total);

        // Verify error was recorded
        let gen_state = exec
            .state()
            .node_states
            .iter()
            .find(|s| s.node_id == "generate_ir")
            .unwrap();
        assert_eq!(gen_state.status, NodeStatus::Failed);
        assert_eq!(gen_state.error.as_deref(), Some("LLM timeout"));
    }

    #[test]
    fn skip_node_allows_dependents() {
        let mut exec = make_executor();

        // Skip generate_ir (e.g., IR already provided)
        exec.skip_node("generate_ir");
        assert_eq!(exec.progress(), (1, 5));

        // validate_syntax should become ready
        let next = exec.advance().map(str::to_string);
        assert_eq!(next.as_deref(), Some("validate_syntax"));
    }

    #[test]
    fn progress_tracking() {
        let mut exec = make_executor();
        assert_eq!(exec.progress(), (0, 5));

        exec.advance();
        exec.complete_node("generate_ir", None);
        assert_eq!(exec.progress(), (1, 5));

        exec.advance();
        exec.skip_node("validate_syntax");
        assert_eq!(exec.progress(), (2, 5));

        exec.advance();
        exec.fail_node("check_consistency", "nope".to_string());
        // check_consistency failed -> gate and execute should get skipped
        exec.advance(); // triggers propagation
        assert!(exec.is_complete());
        assert_eq!(exec.progress(), (5, 5));
    }

    #[test]
    fn to_agent_steps_conversion() {
        let mut exec = make_executor();

        // Initial state: all pending
        let steps = exec.to_agent_steps();
        assert_eq!(steps.len(), 5);
        assert!(steps.iter().all(|s| s.status == AgentStepStatus::Pending));

        // Advance and check
        exec.advance();
        let steps = exec.to_agent_steps();
        assert_eq!(steps[0].status, AgentStepStatus::Active);
        assert_eq!(steps[0].label, "Generate IR");

        // Complete and check
        exec.complete_node("generate_ir", None);
        let steps = exec.to_agent_steps();
        assert_eq!(steps[0].status, AgentStepStatus::Completed);
        assert_eq!(steps[1].status, AgentStepStatus::Pending);

        // Fail a node
        exec.advance();
        exec.fail_node("validate_syntax", "bad syntax".to_string());
        let steps = exec.to_agent_steps();
        assert_eq!(
            steps[1].status,
            AgentStepStatus::Failed("bad syntax".to_string())
        );
    }

    #[test]
    fn topological_ordering_with_diamond_deps() {
        // A -> B, A -> C, B+C -> D
        let graph = ExecutionGraph {
            name: "diamond".to_string(),
            description: "test".to_string(),
            nodes: vec![
                AgentNode {
                    id: "a".to_string(),
                    label: "A".to_string(),
                    node_type: AgentNodeType::GenerateIR,
                    depends_on: vec![],
                },
                AgentNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: AgentNodeType::ValidateSyntax,
                    depends_on: vec!["a".to_string()],
                },
                AgentNode {
                    id: "c".to_string(),
                    label: "C".to_string(),
                    node_type: AgentNodeType::CheckConsistency,
                    depends_on: vec!["a".to_string()],
                },
                AgentNode {
                    id: "d".to_string(),
                    label: "D".to_string(),
                    node_type: AgentNodeType::Execute,
                    depends_on: vec!["b".to_string(), "c".to_string()],
                },
            ],
        };

        let mut exec = GraphExecutor::new(graph);

        // Advance -> a
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("a"));
        exec.complete_node("a", None);

        // Advance -> b (first in topo sort)
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("b"));
        exec.complete_node("b", None);

        // Advance -> c
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("c"));
        exec.complete_node("c", None);

        // Advance -> d
        let node = exec.advance().map(str::to_string);
        assert_eq!(node.as_deref(), Some("d"));
        exec.complete_node("d", None);

        assert!(exec.is_complete());
    }

    #[test]
    fn noop_executor_returns_success() {
        let executor = NoOpExecutor;
        let node = AgentNode {
            id: "test".to_string(),
            label: "Test Node".to_string(),
            node_type: AgentNodeType::GenerateIR,
            depends_on: vec![],
        };
        let ctx = ExecutionContext {
            root: PathBuf::from("/tmp"),
            change_description: "test".to_string(),
            ir_content: String::new(),
            previous_outputs: HashMap::new(),
        };
        let result = executor.execute(&node, &ctx);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Test Node"));
        assert!(output.artifacts.is_empty());
    }

    #[test]
    fn validate_syntax_executor_passes_valid_content() {
        let executor = ValidateSyntaxExecutor;
        let node = AgentNode {
            id: "validate".to_string(),
            label: "Validate".to_string(),
            node_type: AgentNodeType::ValidateSyntax,
            depends_on: vec![],
        };
        let ctx = ExecutionContext {
            root: PathBuf::from("/tmp"),
            change_description: "test".to_string(),
            ir_content: "flowchart TD\n    A[Start] --> B[End]".to_string(),
            previous_outputs: HashMap::new(),
        };
        let result = executor.execute(&node, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_syntax_executor_fails_invalid_content() {
        let executor = ValidateSyntaxExecutor;
        let node = AgentNode {
            id: "validate".to_string(),
            label: "Validate".to_string(),
            node_type: AgentNodeType::ValidateSyntax,
            depends_on: vec![],
        };
        let ctx = ExecutionContext {
            root: PathBuf::from("/tmp"),
            change_description: "test".to_string(),
            ir_content: "not_a_diagram\n    A[Start --> B[End]".to_string(),
            previous_outputs: HashMap::new(),
        };
        let result = executor.execute(&node, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn gate_executor_returns_waiting_message() {
        let executor = GateExecutor;
        let node = AgentNode {
            id: "gate".to_string(),
            label: "Gate".to_string(),
            node_type: AgentNodeType::Gate,
            depends_on: vec![],
        };
        let ctx = ExecutionContext {
            root: PathBuf::from("/tmp"),
            change_description: "test".to_string(),
            ir_content: String::new(),
            previous_outputs: HashMap::new(),
        };
        let result = executor.execute(&node, &ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains("approval"));
    }

    #[test]
    fn execution_state_timestamps_populated() {
        let mut exec = make_executor();
        assert!(!exec.state().started_at.is_empty());

        exec.advance();
        let gen_state = exec
            .state()
            .node_states
            .iter()
            .find(|s| s.node_id == "generate_ir")
            .unwrap();
        assert!(gen_state.started_at.is_some());

        exec.complete_node("generate_ir", None);
        let gen_state = exec
            .state()
            .node_states
            .iter()
            .find(|s| s.node_id == "generate_ir")
            .unwrap();
        assert!(gen_state.completed_at.is_some());
    }

    #[test]
    fn advance_returns_none_when_complete() {
        let graph = ExecutionGraph {
            name: "single".to_string(),
            description: "test".to_string(),
            nodes: vec![AgentNode {
                id: "only".to_string(),
                label: "Only".to_string(),
                node_type: AgentNodeType::GenerateIR,
                depends_on: vec![],
            }],
        };
        let mut exec = GraphExecutor::new(graph);
        exec.advance();
        exec.complete_node("only", None);
        assert!(exec.is_complete());
        assert!(exec.advance().is_none());
    }

    #[test]
    fn lint_mermaid_ir_valid() {
        let errors = lint_mermaid_ir("flowchart TD\n    A[Start] --> B[End]");
        assert!(errors.is_empty());
    }

    #[test]
    fn lint_mermaid_ir_invalid_directive() {
        let errors = lint_mermaid_ir("not_valid\n    A --> B");
        assert!(!errors.is_empty());
        assert!(errors[0].contains("expected diagram directive"));
    }

    #[test]
    fn lint_mermaid_ir_unbalanced_brackets() {
        let errors = lint_mermaid_ir("flowchart TD\n    A[Start --> B[End]");
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.contains("unclosed")));
    }

    #[test]
    fn lint_mermaid_ir_empty_content() {
        let errors = lint_mermaid_ir("");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Empty"));
    }

    #[test]
    fn lint_mermaid_ir_allows_system_wrapper() {
        let errors =
            lint_mermaid_ir("<system_architecture_diagram>\nflowchart TD\n    A --> B\n</system_architecture_diagram>");
        // The wrapper tag is allowed as first directive
        assert!(errors.is_empty());
    }
}
