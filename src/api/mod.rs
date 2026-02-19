//! LLM API integration for llmermaid.
//!
//! This module provides the bridge between the planner's context assembly
//! and an LLM backend. The core types (plan schema, sanitization, validation)
//! are always available. The HTTP client (`client` submodule) requires the
//! `api` cargo feature.
//!
//! # Architecture
//!
//! ```text
//! PlannerContext ─► sanitize ─► LlmRequest ─► client.call() ─► LlmResponse
//!                                                                   │
//!                                          validate ◄── parse plans ◄┘
//! ```
//!
//! The client is the only component that touches the network. Everything
//! else (types, sanitization, validation, tool schema) works offline.

#[cfg(feature = "api")]
pub mod client;
pub mod sanitize;
pub mod tool_schema;
pub mod types;
pub mod validate;

use tracing::info;

use crate::error::{Error, Result};
use crate::planner::types::PlannerContext;

use sanitize::sanitize_for_api;
use tool_schema::{forced_plan_tool_choice, plan_tool_definition, DEFAULT_PLAN_MAX_TOKENS, DEFAULT_PLAN_MODEL, PLAN_TOOL_NAME};
use types::{LlmRequest, Message, PlanToolResponse};

/// Build an [`LlmRequest`] from a [`PlannerContext`].
///
/// Sanitizes the content, assembles the system prompt and user message,
/// and attaches the `submit_plans` tool with forced tool choice.
pub fn build_plan_request(ctx: &PlannerContext, model: Option<&str>) -> LlmRequest {
    let system = sanitize_for_api(crate::planner::PLANNER_SYSTEM_PROMPT);

    let user_content = sanitize_for_api(&crate::planner::render_planning_prompt(ctx));

    let model = model.unwrap_or(DEFAULT_PLAN_MODEL).to_string();

    info!(
        model,
        scopes = ctx.relevant_scopes.len(),
        tokens_est = ctx.total_tokens_est,
        "Building plan request"
    );

    LlmRequest {
        system,
        messages: vec![Message {
            role: "user".to_string(),
            content: user_content,
        }],
        tools: vec![plan_tool_definition()],
        tool_choice: forced_plan_tool_choice(),
        model,
        max_tokens: DEFAULT_PLAN_MAX_TOKENS,
    }
}

/// Parse the plan tool response from an [`LlmResponse`].
///
/// Extracts the `submit_plans` tool use block and deserializes it
/// into a [`PlanToolResponse`].
pub fn parse_plan_response(
    response: &types::LlmResponse,
) -> Result<PlanToolResponse> {
    let input = response.extract_tool_input(PLAN_TOOL_NAME).ok_or_else(|| {
        Error::Api {
            status: None,
            message: format!(
                "API response did not contain a '{}' tool use block. Stop reason: {:?}",
                PLAN_TOOL_NAME, response.stop_reason
            ),
        }
    })?;

    let plan_response: PlanToolResponse =
        serde_json::from_value(input.clone()).map_err(|e| Error::Api {
            status: None,
            message: format!("Failed to parse plan tool response: {e}"),
        })?;

    if plan_response.plans.is_empty() {
        return Err(Error::Api {
            status: None,
            message: "API returned empty plans array".to_string(),
        });
    }

    info!(
        variants = plan_response.plans.len(),
        "Parsed plan response"
    );

    Ok(plan_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::types::ManifestEntry;

    fn sample_context() -> PlannerContext {
        PlannerContext {
            change_description: "Add a locations table".to_string(),
            relevant_scopes: vec!["supabase".to_string()],
            system_diagram: "flowchart LR\n  A-->B".to_string(),
            selected_diagrams: vec![(
                ManifestEntry {
                    id: "db-schema".to_string(),
                    source: "docs/db.mmd".to_string(),
                    scope: "supabase".to_string(),
                    diagram_type: "erDiagram".to_string(),
                    tokens_est: 100,
                },
                "erDiagram\n  User".to_string(),
            )],
            relevant_claude_mds: vec![],
            file_index_excerpt: "files[1]{path,type,domain}:\nschema.sql,migration,supabase"
                .to_string(),
            total_tokens_est: 300,
        }
    }

    #[test]
    fn build_plan_request_default_model() {
        let ctx = sample_context();
        let req = build_plan_request(&ctx, None);
        assert_eq!(req.model, DEFAULT_PLAN_MODEL);
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].name, PLAN_TOOL_NAME);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
    }

    #[test]
    fn build_plan_request_custom_model() {
        let ctx = sample_context();
        let req = build_plan_request(&ctx, Some("claude-sonnet-4-6"));
        assert_eq!(req.model, "claude-sonnet-4-6");
    }

    #[test]
    fn build_plan_request_sanitizes_content() {
        let mut ctx = sample_context();
        ctx.change_description =
            "Fix the ANTHROPIC_API_KEY=sk-ant-secret123456789012345 leak".to_string();
        let req = build_plan_request(&ctx, None);
        assert!(!req.messages[0].content.contains("sk-ant-secret"));
    }

    #[test]
    fn parse_plan_response_success() {
        let response = types::LlmResponse {
            id: "msg_123".to_string(),
            content: vec![types::ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: PLAN_TOOL_NAME.to_string(),
                input: serde_json::json!({
                    "plans": [{
                        "title": "Add locations",
                        "summary": "Create a locations table",
                        "affected_boundaries": ["supabase"],
                        "diagram_changes": [],
                        "files_to_change": [{
                            "path": "migrations/001.sql",
                            "action": "create",
                            "description": "Create locations table"
                        }]
                    }]
                }),
            }],
            model: "claude-opus-4-6".to_string(),
            usage: types::Usage {
                input_tokens: 2000,
                output_tokens: 500,
            },
            stop_reason: Some("tool_use".to_string()),
        };

        let result = parse_plan_response(&response);
        assert!(result.is_ok());
        let plans = result.unwrap();
        assert_eq!(plans.plans.len(), 1);
        assert_eq!(plans.plans[0].title, "Add locations");
    }

    #[test]
    fn parse_plan_response_missing_tool() {
        let response = types::LlmResponse {
            id: "msg_123".to_string(),
            content: vec![types::ContentBlock::Text {
                text: "I can't use tools.".to_string(),
            }],
            model: "claude-opus-4-6".to_string(),
            usage: types::Usage {
                input_tokens: 100,
                output_tokens: 20,
            },
            stop_reason: Some("end_turn".to_string()),
        };

        let result = parse_plan_response(&response);
        assert!(result.is_err());
    }

    #[test]
    fn parse_plan_response_empty_plans() {
        let response = types::LlmResponse {
            id: "msg_123".to_string(),
            content: vec![types::ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: PLAN_TOOL_NAME.to_string(),
                input: serde_json::json!({"plans": []}),
            }],
            model: "claude-opus-4-6".to_string(),
            usage: types::Usage {
                input_tokens: 100,
                output_tokens: 20,
            },
            stop_reason: Some("tool_use".to_string()),
        };

        let result = parse_plan_response(&response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }
}
