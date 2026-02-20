//! Agent execution engine for llmermaid.
//!
//! Models the Plan-Verify-Execute workflow as a directed acyclic graph (DAG)
//! of typed nodes with dependency edges. The [`ExecutionGraph`] defines the
//! static structure; [`ExecutionState`] tracks runtime progress.
//!
//! # Design
//!
//! Nodes declare their dependencies via `depends_on` (IDs of prerequisite
//! nodes). The executor only marks a node as [`NodeStatus::Ready`] once every
//! dependency has reached [`NodeStatus::Completed`] or [`NodeStatus::Skipped`].
//! Failed nodes propagate: any node whose transitive dependency set contains a
//! failure is itself marked [`NodeStatus::Skipped`] when the executor advances.

pub mod executor;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Discriminant for the kind of work a node performs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentNodeType {
    /// Generate mermaid IR from change description.
    GenerateIR,
    /// Validate mermaid syntax.
    ValidateSyntax,
    /// Check diagram consistency with existing codebase.
    CheckConsistency,
    /// Apply repairs to fix syntax issues.
    RepairSyntax,
    /// Execute the plan (write files, update diagrams).
    Execute,
    /// User verification gate (requires approval).
    Gate,
    /// Custom/extension node.
    Custom(String),
}

/// A node in the agent execution graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    /// Unique identifier within the graph.
    pub id: String,
    /// Human-readable label (shown in TUI step tracker).
    pub label: String,
    /// What kind of work this node performs.
    pub node_type: AgentNodeType,
    /// IDs of prerequisite nodes that must complete before this one can run.
    pub depends_on: Vec<String>,
}

// ---------------------------------------------------------------------------
// Execution graph (static structure)
// ---------------------------------------------------------------------------

/// The full execution graph -- a DAG of [`AgentNode`]s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGraph {
    /// Ordered list of nodes. Insertion order need not be topological.
    pub nodes: Vec<AgentNode>,
    /// Short name for this graph (e.g. "plan-verify-execute").
    pub name: String,
    /// Longer description of the graph's purpose.
    pub description: String,
}

impl ExecutionGraph {
    /// Create the default plan-verify-execute graph.
    ///
    /// ```text
    /// generate_ir -> validate_syntax -> gate -> execute
    ///                                      \-> check_consistency -> execute
    /// ```
    ///
    /// The `gate` node pauses execution until the user approves.
    pub fn default_plan_graph() -> Self {
        Self {
            name: "plan-verify-execute".to_string(),
            description: "Generate IR, validate, gate-check, then execute.".to_string(),
            nodes: vec![
                AgentNode {
                    id: "generate_ir".to_string(),
                    label: "Generate IR".to_string(),
                    node_type: AgentNodeType::GenerateIR,
                    depends_on: vec![],
                },
                AgentNode {
                    id: "validate_syntax".to_string(),
                    label: "Validate Syntax".to_string(),
                    node_type: AgentNodeType::ValidateSyntax,
                    depends_on: vec!["generate_ir".to_string()],
                },
                AgentNode {
                    id: "check_consistency".to_string(),
                    label: "Check Consistency".to_string(),
                    node_type: AgentNodeType::CheckConsistency,
                    depends_on: vec!["validate_syntax".to_string()],
                },
                AgentNode {
                    id: "gate".to_string(),
                    label: "User Approval".to_string(),
                    node_type: AgentNodeType::Gate,
                    depends_on: vec!["check_consistency".to_string()],
                },
                AgentNode {
                    id: "execute".to_string(),
                    label: "Execute Plan".to_string(),
                    node_type: AgentNodeType::Execute,
                    depends_on: vec!["gate".to_string()],
                },
            ],
        }
    }

    /// Create a repair-focused graph (lint -> fix -> validate loop).
    ///
    /// ```text
    /// validate_syntax -> repair_syntax -> validate_repaired
    /// ```
    pub fn repair_graph() -> Self {
        Self {
            name: "repair".to_string(),
            description: "Lint existing diagrams, repair syntax errors, re-validate.".to_string(),
            nodes: vec![
                AgentNode {
                    id: "validate_syntax".to_string(),
                    label: "Lint Diagrams".to_string(),
                    node_type: AgentNodeType::ValidateSyntax,
                    depends_on: vec![],
                },
                AgentNode {
                    id: "repair_syntax".to_string(),
                    label: "Repair Syntax".to_string(),
                    node_type: AgentNodeType::RepairSyntax,
                    depends_on: vec!["validate_syntax".to_string()],
                },
                AgentNode {
                    id: "validate_repaired".to_string(),
                    label: "Re-validate".to_string(),
                    node_type: AgentNodeType::ValidateSyntax,
                    depends_on: vec!["repair_syntax".to_string()],
                },
            ],
        }
    }

    /// Return node IDs in topological order (Kahn's algorithm).
    ///
    /// Panics only if the graph contains a cycle, which is a programmer error
    /// in the graph builders above.
    pub fn topological_order(&self) -> Vec<&str> {
        use std::collections::{HashMap, VecDeque};

        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for node in &self.nodes {
            in_degree.entry(node.id.as_str()).or_insert(0);
            adjacency.entry(node.id.as_str()).or_default();
            for dep in &node.depends_on {
                adjacency.entry(dep.as_str()).or_default().push(node.id.as_str());
                *in_degree.entry(node.id.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        // Deterministic ordering: sort the initial queue
        let mut initial: Vec<&str> = queue.drain(..).collect();
        initial.sort();
        queue.extend(initial);

        let mut order: Vec<&str> = Vec::with_capacity(self.nodes.len());

        while let Some(current) = queue.pop_front() {
            order.push(current);
            if let Some(dependents) = adjacency.get(current) {
                let mut next_ready: Vec<&str> = Vec::new();
                for &dep in dependents {
                    let deg = in_degree.get_mut(dep).expect("node in adjacency but not in_degree");
                    *deg -= 1;
                    if *deg == 0 {
                        next_ready.push(dep);
                    }
                }
                // Sort for determinism when multiple nodes become ready simultaneously
                next_ready.sort();
                queue.extend(next_ready);
            }
        }

        assert_eq!(
            order.len(),
            self.nodes.len(),
            "cycle detected in execution graph"
        );

        order
    }

    /// Find node IDs whose dependencies are all satisfied (completed or skipped).
    ///
    /// Only returns nodes that are currently in [`NodeStatus::Pending`] state.
    pub fn ready_nodes<'a>(&'a self, states: &[NodeState]) -> Vec<&'a str> {
        let status_map: std::collections::HashMap<&str, &NodeStatus> = states
            .iter()
            .map(|s| (s.node_id.as_str(), &s.status))
            .collect();

        self.nodes
            .iter()
            .filter(|node| {
                // Must be Pending to become Ready
                let current = status_map.get(node.id.as_str());
                if !matches!(current, Some(NodeStatus::Pending)) {
                    return false;
                }
                // All dependencies must be Completed or Skipped
                node.depends_on.iter().all(|dep_id| {
                    matches!(
                        status_map.get(dep_id.as_str()),
                        Some(NodeStatus::Completed) | Some(NodeStatus::Skipped)
                    )
                })
            })
            .map(|node| node.id.as_str())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

/// Runtime status of a single node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    /// Not yet eligible to run.
    Pending,
    /// All dependencies satisfied; eligible for execution.
    Ready,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with error.
    Failed,
    /// Explicitly skipped (e.g. repair not needed, or blocked by upstream failure).
    Skipped,
    /// Paused at a user-approval gate.
    WaitingForGate,
}

/// Runtime state of a single node.
#[derive(Debug, Clone)]
pub struct NodeState {
    /// ID matching [`AgentNode::id`].
    pub node_id: String,
    /// Current status.
    pub status: NodeStatus,
    /// ISO-8601 timestamp when execution started.
    pub started_at: Option<String>,
    /// ISO-8601 timestamp when execution completed.
    pub completed_at: Option<String>,
    /// Output produced by this node (if completed).
    pub output: Option<String>,
    /// Error message (if failed).
    pub error: Option<String>,
}

impl NodeState {
    /// Create a new `NodeState` in [`NodeStatus::Pending`].
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            status: NodeStatus::Pending,
            started_at: None,
            completed_at: None,
            output: None,
            error: None,
        }
    }
}

/// The runtime state of the entire execution.
#[derive(Debug, Clone)]
pub struct ExecutionState {
    /// The static graph structure.
    pub graph: ExecutionGraph,
    /// Per-node runtime state, indexed in the same order as `graph.nodes`.
    pub node_states: Vec<NodeState>,
    /// ID of the node currently being executed, if any.
    pub current_node: Option<String>,
    /// `true` once every node is completed, failed, or skipped.
    pub is_complete: bool,
    /// ISO-8601 timestamp when execution started.
    pub started_at: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plan_graph_has_five_nodes() {
        let g = ExecutionGraph::default_plan_graph();
        assert_eq!(g.nodes.len(), 5);
        assert_eq!(g.name, "plan-verify-execute");
    }

    #[test]
    fn repair_graph_has_three_nodes() {
        let g = ExecutionGraph::repair_graph();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.name, "repair");
    }

    #[test]
    fn topological_order_default_plan() {
        let g = ExecutionGraph::default_plan_graph();
        let order = g.topological_order();
        assert_eq!(
            order,
            vec![
                "generate_ir",
                "validate_syntax",
                "check_consistency",
                "gate",
                "execute",
            ]
        );
    }

    #[test]
    fn topological_order_repair() {
        let g = ExecutionGraph::repair_graph();
        let order = g.topological_order();
        assert_eq!(
            order,
            vec!["validate_syntax", "repair_syntax", "validate_repaired"]
        );
    }

    #[test]
    fn topological_order_diamond_dependencies() {
        // A -> B, A -> C, B -> D, C -> D  (diamond)
        let g = ExecutionGraph {
            name: "diamond".to_string(),
            description: "Diamond dependency test".to_string(),
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
        let order = g.topological_order();
        // "a" must come first, "d" must come last
        assert_eq!(order[0], "a");
        assert_eq!(order[3], "d");
        // "b" and "c" can be in either order, but deterministic sort gives b before c
        assert_eq!(order[1], "b");
        assert_eq!(order[2], "c");
    }

    #[test]
    fn ready_nodes_initial_state() {
        let g = ExecutionGraph::default_plan_graph();
        let states: Vec<NodeState> = g
            .nodes
            .iter()
            .map(|n| NodeState::new(&n.id))
            .collect();
        let ready = g.ready_nodes(&states);
        // Only the root node (no deps) should be ready
        assert_eq!(ready, vec!["generate_ir"]);
    }

    #[test]
    fn ready_nodes_after_first_completion() {
        let g = ExecutionGraph::default_plan_graph();
        let mut states: Vec<NodeState> = g
            .nodes
            .iter()
            .map(|n| NodeState::new(&n.id))
            .collect();
        // Complete the first node
        states[0].status = NodeStatus::Completed;
        let ready = g.ready_nodes(&states);
        assert_eq!(ready, vec!["validate_syntax"]);
    }

    #[test]
    fn ready_nodes_skipped_dep_satisfies() {
        let g = ExecutionGraph::default_plan_graph();
        let mut states: Vec<NodeState> = g
            .nodes
            .iter()
            .map(|n| NodeState::new(&n.id))
            .collect();
        // Skip the first node
        states[0].status = NodeStatus::Skipped;
        let ready = g.ready_nodes(&states);
        assert_eq!(ready, vec!["validate_syntax"]);
    }

    #[test]
    fn ready_nodes_failed_dep_blocks() {
        let g = ExecutionGraph::default_plan_graph();
        let mut states: Vec<NodeState> = g
            .nodes
            .iter()
            .map(|n| NodeState::new(&n.id))
            .collect();
        // Fail the first node
        states[0].status = NodeStatus::Failed;
        let ready = g.ready_nodes(&states);
        // Nothing should be ready -- failed dep does not satisfy
        assert!(ready.is_empty());
    }

    #[test]
    fn ready_nodes_diamond() {
        // Diamond: A -> B, A -> C, B+C -> D
        let g = ExecutionGraph {
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

        let mut states: Vec<NodeState> = g
            .nodes
            .iter()
            .map(|n| NodeState::new(&n.id))
            .collect();

        // Complete A -> B and C become ready
        states[0].status = NodeStatus::Completed;
        let ready = g.ready_nodes(&states);
        assert_eq!(ready, vec!["b", "c"]);

        // Complete B only -> D is NOT ready (C still pending)
        states[1].status = NodeStatus::Completed;
        let ready = g.ready_nodes(&states);
        assert_eq!(ready, vec!["c"]);

        // Complete C -> D is now ready
        states[2].status = NodeStatus::Completed;
        let ready = g.ready_nodes(&states);
        assert_eq!(ready, vec!["d"]);
    }

    #[test]
    fn node_state_new_defaults() {
        let s = NodeState::new("test_node");
        assert_eq!(s.node_id, "test_node");
        assert_eq!(s.status, NodeStatus::Pending);
        assert!(s.started_at.is_none());
        assert!(s.completed_at.is_none());
        assert!(s.output.is_none());
        assert!(s.error.is_none());
    }

    #[test]
    fn agent_node_type_equality() {
        assert_eq!(AgentNodeType::GenerateIR, AgentNodeType::GenerateIR);
        assert_ne!(AgentNodeType::GenerateIR, AgentNodeType::ValidateSyntax);
        assert_eq!(
            AgentNodeType::Custom("foo".to_string()),
            AgentNodeType::Custom("foo".to_string())
        );
        assert_ne!(
            AgentNodeType::Custom("foo".to_string()),
            AgentNodeType::Custom("bar".to_string())
        );
    }

    #[test]
    fn execution_graph_serialization_roundtrip() {
        let g = ExecutionGraph::default_plan_graph();
        let json = serde_json::to_string(&g).expect("serialize");
        let g2: ExecutionGraph = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(g2.nodes.len(), g.nodes.len());
        assert_eq!(g2.name, g.name);
        for (a, b) in g.nodes.iter().zip(g2.nodes.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.node_type, b.node_type);
            assert_eq!(a.depends_on, b.depends_on);
        }
    }
}
