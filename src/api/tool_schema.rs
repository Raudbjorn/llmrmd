//! PLAN_TOOL definition for structured output via Anthropic tool use.
//!
//! Forces the model to return a JSON object matching [`PlanToolResponse`]
//! by using `tool_choice = {"type": "tool", "name": "submit_plans"}`.

use super::types::{ToolChoice, ToolDefinition};

/// The tool name used for forced structured output.
pub const PLAN_TOOL_NAME: &str = "submit_plans";

/// Default model for planning.
pub const DEFAULT_PLAN_MODEL: &str = "claude-opus-4-6";

/// Default max tokens for plan responses.
pub const DEFAULT_PLAN_MAX_TOKENS: u32 = 8192;

/// Build the `submit_plans` tool definition.
///
/// The JSON schema enforces the structure that [`PlanToolResponse`] expects,
/// ensuring the model returns valid, parseable plans.
pub fn plan_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: PLAN_TOOL_NAME.to_string(),
        description: "Submit one or more implementation plans for the proposed change. \
            Each plan must include a title, summary, affected boundaries, file changes, \
            and diagram changes. Use multiple plans for genuinely different approaches."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "required": ["plans"],
            "properties": {
                "plans": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "required": [
                            "title", "summary", "affected_boundaries",
                            "files_to_change", "diagram_changes"
                        ],
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Short descriptive title for this plan variant"
                            },
                            "summary": {
                                "type": "string",
                                "description": "2-3 sentence summary of the approach"
                            },
                            "affected_boundaries": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Which domain boundaries this change crosses"
                            },
                            "context_pointers": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["path", "reason"],
                                    "properties": {
                                        "path": { "type": "string" },
                                        "reason": { "type": "string" }
                                    }
                                },
                                "description": "Files the implementing LLM needs in its context window"
                            },
                            "edge_changes": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["from", "to", "action"],
                                    "properties": {
                                        "from": { "type": "string" },
                                        "to": { "type": "string" },
                                        "action": {
                                            "type": "string",
                                            "enum": ["add", "remove", "modify"]
                                        },
                                        "label": { "type": "string" }
                                    }
                                },
                                "description": "Edges to add/remove/modify in architecture diagrams"
                            },
                            "diagram_changes": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["diagram_id", "action", "proposed_mermaid"],
                                    "properties": {
                                        "diagram_id": { "type": "string" },
                                        "action": {
                                            "type": "string",
                                            "enum": ["create", "modify"]
                                        },
                                        "proposed_mermaid": { "type": "string" }
                                    }
                                },
                                "description": "Updated or new mermaid diagrams"
                            },
                            "files_to_change": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["path", "action", "description"],
                                    "properties": {
                                        "path": { "type": "string" },
                                        "action": {
                                            "type": "string",
                                            "enum": ["create", "modify", "delete", "move"]
                                        },
                                        "description": { "type": "string" },
                                        "depends_on": {
                                            "type": "array",
                                            "items": { "type": "integer" }
                                        }
                                    }
                                },
                                "description": "Ordered list of file changes to implement"
                            },
                            "files_to_avoid": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Files that should NOT be modified"
                            },
                            "edge_violations": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Any layer policy violations this plan introduces"
                            },
                            "risks": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Potential risks or breaking changes"
                            },
                            "uncertainty": {
                                "type": "string",
                                "enum": ["low", "medium", "high"],
                                "description": "Overall confidence level in this plan"
                            }
                        }
                    }
                }
            }
        }),
    }
}

/// Build the tool choice that forces use of the submit_plans tool.
pub fn forced_plan_tool_choice() -> ToolChoice {
    ToolChoice::Tool {
        name: PLAN_TOOL_NAME.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_tool_has_correct_name() {
        let tool = plan_tool_definition();
        assert_eq!(tool.name, "submit_plans");
    }

    #[test]
    fn plan_tool_schema_has_plans_array() {
        let tool = plan_tool_definition();
        let props = tool.input_schema.get("properties").unwrap();
        let plans = props.get("plans").unwrap();
        assert_eq!(plans["type"], "array");
        assert_eq!(plans["minItems"], 1);
    }

    #[test]
    fn plan_tool_schema_has_required_fields() {
        let tool = plan_tool_definition();
        let items = &tool.input_schema["properties"]["plans"]["items"];
        let required: Vec<&str> = items["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();

        assert!(required.contains(&"title"));
        assert!(required.contains(&"summary"));
        assert!(required.contains(&"affected_boundaries"));
        assert!(required.contains(&"files_to_change"));
        assert!(required.contains(&"diagram_changes"));
    }

    #[test]
    fn forced_tool_choice_serializes_correctly() {
        let choice = forced_plan_tool_choice();
        let json = serde_json::to_value(&choice).unwrap();
        assert_eq!(json["type"], "tool");
        assert_eq!(json["name"], "submit_plans");
    }

    #[test]
    fn plan_tool_file_action_enum_values() {
        let tool = plan_tool_definition();
        let file_action = &tool.input_schema["properties"]["plans"]["items"]["properties"]
            ["files_to_change"]["items"]["properties"]["action"]["enum"];
        let actions: Vec<&str> = file_action
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(actions, vec!["create", "modify", "delete", "move"]);
    }

    #[test]
    fn plan_tool_uncertainty_enum_values() {
        let tool = plan_tool_definition();
        let uncertainty = &tool.input_schema["properties"]["plans"]["items"]["properties"]
            ["uncertainty"]["enum"];
        let values: Vec<&str> = uncertainty
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(values, vec!["low", "medium", "high"]);
    }
}
