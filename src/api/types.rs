//! Types for LLM API communication.
//!
//! These types model the Anthropic Messages API but are designed to be
//! provider-agnostic at the trait boundary. Only the client implementation
//! is Anthropic-specific.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A tool definition sent to the API.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Tool choice strategy.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ToolChoice {
    /// Force the model to use a specific tool.
    #[serde(rename = "tool")]
    Tool { name: String },
    /// Let the model decide whether to use tools.
    #[serde(rename = "auto")]
    Auto,
    /// Force the model to use any tool.
    #[serde(rename = "any")]
    Any,
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Complete request to the LLM backend.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: ToolChoice,
    pub model: String,
    pub max_tokens: u32,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A content block in the API response.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Token usage from the API response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Raw API response from Anthropic Messages API.
#[derive(Debug, Clone, Deserialize)]
pub struct LlmResponse {
    pub id: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Plan types (provider-independent)
// ---------------------------------------------------------------------------

/// A single file change step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub action: FileAction,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<usize>,
}

/// Action to perform on a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileAction {
    Create,
    Modify,
    Delete,
    Move,
}

/// A context pointer — file the implementing LLM needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPointer {
    pub path: String,
    pub reason: String,
}

/// An edge change in architecture diagrams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeChange {
    pub from: String,
    pub to: String,
    pub action: EdgeAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Action for an edge in the architecture graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EdgeAction {
    Add,
    Remove,
    Modify,
}

/// A diagram change (new or modified mermaid diagram).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramChange {
    pub diagram_id: String,
    pub action: DiagramAction,
    pub proposed_mermaid: String,
}

/// Action for a diagram.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DiagramAction {
    Create,
    Modify,
}

/// Confidence level for a plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Uncertainty {
    Low,
    Medium,
    High,
}

/// A single plan variant returned by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub summary: String,
    pub affected_boundaries: Vec<String>,
    #[serde(default)]
    pub context_pointers: Vec<ContextPointer>,
    #[serde(default)]
    pub edge_changes: Vec<EdgeChange>,
    pub diagram_changes: Vec<DiagramChange>,
    pub files_to_change: Vec<FileChange>,
    #[serde(default)]
    pub files_to_avoid: Vec<String>,
    #[serde(default)]
    pub edge_violations: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub uncertainty: Option<Uncertainty>,
}

/// The top-level tool response wrapping one or more plan variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanToolResponse {
    pub plans: Vec<Plan>,
}

/// Status of a persisted plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Draft,
    Approved,
    Rejected,
    Superseded,
}

/// Summary of the context used when generating a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSummary {
    pub scopes: Vec<String>,
    pub diagram_count: usize,
    pub token_estimate: usize,
}

/// A persisted plan with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedPlan {
    pub slug: String,
    pub change_description: String,
    pub status: PlanStatus,
    pub created_at: String,
    pub model: String,
    pub usage: Option<Usage>,
    pub plans: Vec<Plan>,
    pub context_summary: ContextSummary,
    #[serde(default)]
    pub validation_warnings: Vec<String>,
}

impl LlmResponse {
    /// Extract the tool use input from the response (first tool_use block).
    pub fn extract_tool_input(&self, tool_name: &str) -> Option<&serde_json::Value> {
        self.content.iter().find_map(|block| match block {
            ContentBlock::ToolUse { name, input, .. } if name == tool_name => Some(input),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_action_serde_roundtrip() {
        let action = FileAction::Create;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, "\"create\"");
        let parsed: FileAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, FileAction::Create);
    }

    #[test]
    fn uncertainty_serde_roundtrip() {
        let u = Uncertainty::High;
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(json, "\"high\"");
        let parsed: Uncertainty = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Uncertainty::High);
    }

    #[test]
    fn plan_status_serde_roundtrip() {
        let s = PlanStatus::Approved;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"approved\"");
        let parsed: PlanStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, PlanStatus::Approved);
    }

    #[test]
    fn plan_tool_response_deserialize() {
        let json = serde_json::json!({
            "plans": [{
                "title": "Add locations table",
                "summary": "Create a new locations table with RLS",
                "affected_boundaries": ["supabase", "web"],
                "context_pointers": [{"path": "schema.sql", "reason": "existing schema"}],
                "edge_changes": [{"from": "web", "to": "supabase", "action": "add"}],
                "diagram_changes": [{"diagram_id": "db-schema", "action": "modify", "proposed_mermaid": "erDiagram\n  Location"}],
                "files_to_change": [{"path": "migrations/001.sql", "action": "create", "description": "Create table"}],
                "files_to_avoid": ["auth.sql"],
                "edge_violations": [],
                "risks": ["May need backfill"],
                "uncertainty": "low"
            }]
        });

        let response: PlanToolResponse = serde_json::from_value(json).unwrap();
        assert_eq!(response.plans.len(), 1);
        assert_eq!(response.plans[0].title, "Add locations table");
        assert_eq!(response.plans[0].uncertainty, Some(Uncertainty::Low));
        assert_eq!(response.plans[0].files_to_change[0].action, FileAction::Create);
    }

    #[test]
    fn extract_tool_input_finds_matching_block() {
        let response = LlmResponse {
            id: "msg_123".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Here's my plan.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "submit_plans".to_string(),
                    input: serde_json::json!({"plans": []}),
                },
            ],
            model: "claude-opus-4-6".to_string(),
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
            },
            stop_reason: Some("tool_use".to_string()),
        };

        let input = response.extract_tool_input("submit_plans");
        assert!(input.is_some());
        assert!(input.unwrap().get("plans").is_some());
    }

    #[test]
    fn extract_tool_input_returns_none_for_missing() {
        let response = LlmResponse {
            id: "msg_123".to_string(),
            content: vec![ContentBlock::Text {
                text: "No tools.".to_string(),
            }],
            model: "claude-opus-4-6".to_string(),
            usage: Usage {
                input_tokens: 50,
                output_tokens: 20,
            },
            stop_reason: Some("end_turn".to_string()),
        };

        assert!(response.extract_tool_input("submit_plans").is_none());
    }

    #[test]
    fn tool_choice_serializes_correctly() {
        let forced = ToolChoice::Tool {
            name: "submit_plans".to_string(),
        };
        let json = serde_json::to_value(&forced).unwrap();
        assert_eq!(json["type"], "tool");
        assert_eq!(json["name"], "submit_plans");

        let auto = ToolChoice::Auto;
        let json = serde_json::to_value(&auto).unwrap();
        assert_eq!(json["type"], "auto");
    }

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, "user");
        assert_eq!(parsed.content, "Hello");
    }
}
