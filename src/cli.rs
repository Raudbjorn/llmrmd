//! Claude CLI execution module.
//!
//! Spawns the `claude` binary for both streaming and non-streaming requests.
//! Adapted from laude's CLI execution layer, using blocking I/O to match
//! llmermaid's synchronous architecture.
//!
//! # Architecture
//!
//! ```text
//! CliRequest ─► spawn_cli() ─► stdout lines ─► parse events ─► CliResponse
//!                                    │                              │
//!                              (stream-json)              (streaming: mpsc)
//!                                    │                              │
//!                              (json)                   (blocking: collected)
//! ```

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::{Error, Result};

// ── CLI Request/Response Types ──────────────────────────────────────────────

/// Input for a CLI execution.
#[derive(Debug, Clone)]
pub struct CliRequest {
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub model: String,
    pub output_format: OutputFormat,
    pub dangerously_skip_permissions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Json,
    StreamJson,
}

/// Response from a non-streaming CLI execution.
#[derive(Debug, Clone)]
pub struct CliResponse {
    pub text: String,
    pub cost_usd: Option<f64>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

/// JSON output from `claude -p --output-format json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeCliOutput {
    #[serde(rename = "type")]
    response_type: Option<String>,
    result: Option<String>,
    cost_usd: Option<f64>,
    is_error: Option<bool>,
    usage: Option<serde_json::Value>,
    message: Option<serde_json::Value>,
    subtype: Option<String>,
}

// ── Streaming Event Types ───────────────────────────────────────────────────

/// Events emitted during streaming CLI execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { model: String, input_tokens: u32 },

    #[serde(rename = "content_block_start")]
    ContentBlockStart { index: u32 },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: Delta },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },

    #[serde(rename = "message_delta")]
    MessageDelta {
        stop_reason: Option<String>,
        output_tokens: u32,
    },

    #[serde(rename = "message_stop")]
    MessageStop,

    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

// ── Binary Location ─────────────────────────────────────────────────────────

/// Find the claude binary, returning an error if not found.
pub fn claude_binary() -> Result<PathBuf> {
    which::which("claude").map_err(|_| {
        Error::Config(
            "claude binary not found. Install with: npm install -g @anthropic-ai/claude-code\n\
             Or run without --skip-checks to be prompted for installation."
                .to_string(),
        )
    })
}

// ── Non-Streaming Execution ─────────────────────────────────────────────────

/// Execute a CLI request and return the complete response.
pub fn execute(req: &CliRequest) -> Result<CliResponse> {
    let binary = claude_binary()?;
    let args = build_args(req, false);
    debug!(?binary, ?args, "spawning claude CLI (non-streaming)");

    let output = Command::new(&binary)
        .args(&args)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| Error::Terminal(format!("failed to spawn claude CLI: {e}")))?;

    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if !stderr_str.is_empty() {
        debug!(stderr = %stderr_str, "CLI stderr");
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if stdout_str.trim().is_empty() {
        return Err(Error::Terminal(format!(
            "empty output from CLI. stderr: {stderr_str}"
        )));
    }

    let result_json = parse_cli_output(&stdout_str)?;

    let cli_output: ClaudeCliOutput =
        serde_json::from_value(result_json.clone()).map_err(|e| Error::Terminal(format!(
            "failed to deserialize CLI result: {e}"
        )))?;

    if cli_output.is_error == Some(true) {
        let msg = cli_output.result.unwrap_or_else(|| "unknown CLI error".into());
        return Err(Error::Terminal(format!("CLI error: {msg}")));
    }

    let text = cli_output.result.unwrap_or_default();
    let cost_usd = cli_output.cost_usd;

    let usage_obj = result_json.get("usage");
    let input_tokens = usage_obj
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| estimate_tokens(&req.prompt) as u64) as u32;
    let output_tokens = usage_obj
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| estimate_tokens(&text) as u64) as u32;
    let cache_creation = usage_obj
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let cache_read = usage_obj
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    debug!(cost_usd = ?cost_usd, input_tokens, output_tokens, "CLI response received");

    Ok(CliResponse {
        text,
        cost_usd,
        input_tokens,
        output_tokens,
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
    })
}

// ── Streaming Execution ─────────────────────────────────────────────────────

/// Execute a CLI request with streaming, returning a receiver of stream events.
///
/// The CLI is spawned with `--output-format stream-json --verbose` and its
/// stdout is parsed line-by-line on a background thread. Events are sent
/// through the returned `mpsc::Receiver`.
///
/// The background thread terminates when the CLI process exits or on timeout.
pub fn execute_streaming(req: &CliRequest) -> Result<mpsc::Receiver<StreamEvent>> {
    let binary = claude_binary()?;
    let args = build_args(req, true);
    debug!(?binary, ?args, "spawning claude CLI (streaming)");

    let mut child = Command::new(&binary)
        .args(&args)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Terminal(format!("failed to spawn claude CLI: {e}")))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take();

    let (tx, rx) = mpsc::channel::<StreamEvent>();

    let model = req.model.clone();
    let prompt_text = req.prompt.clone();

    std::thread::spawn(move || {
        // Drain stderr in a separate thread
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let mut reader = BufReader::new(stderr);
                let _ = reader.read_to_string(&mut buf);
                if !buf.is_empty() {
                    debug!(stderr = %buf, "CLI stderr (streaming)");
                }
            });
        }

        let reader = BufReader::new(stdout);
        let input_tokens = estimate_tokens(&prompt_text);

        // Send message_start
        let _ = tx.send(StreamEvent::MessageStart {
            model: model.clone(),
            input_tokens,
        });

        let mut block_started = false;
        let block_index: u32 = 0;
        let mut total_text = String::new();
        let mut sent_stop = false;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    warn!(error = %e, "error reading CLI stdout");
                    break;
                }
            };

            if line.is_empty() {
                continue;
            }

            let event: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    debug!(error = %e, raw = %line, "skipping unparseable CLI line");
                    continue;
                }
            };

            let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match event_type {
                "assistant" => {
                    if let Some(content) = event
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content {
                            let block_type =
                                block.get("type").and_then(|v| v.as_str()).unwrap_or("");

                            if block_type == "text" {
                                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        if !block_started {
                                            let _ = tx.send(StreamEvent::ContentBlockStart {
                                                index: block_index,
                                            });
                                            block_started = true;
                                        }

                                        total_text.push_str(text);

                                        let _ = tx.send(StreamEvent::ContentBlockDelta {
                                            index: block_index,
                                            delta: Delta::TextDelta {
                                                text: text.to_string(),
                                            },
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                "result" => {
                    if block_started {
                        let _ = tx.send(StreamEvent::ContentBlockStop {
                            index: block_index,
                        });
                    }

                    let output_tokens = estimate_tokens(&total_text);

                    let _ = tx.send(StreamEvent::MessageDelta {
                        stop_reason: Some("end_turn".into()),
                        output_tokens,
                    });

                    let _ = tx.send(StreamEvent::MessageStop);
                    sent_stop = true;
                    break;
                }
                _ => {
                    debug!(event_type, "ignoring CLI stream event");
                }
            }
        }

        // Send close events if we didn't get a proper result
        if !sent_stop {
            if block_started {
                let _ = tx.send(StreamEvent::ContentBlockStop {
                    index: block_index,
                });
            }
            let output_tokens = estimate_tokens(&total_text);
            let _ = tx.send(StreamEvent::MessageDelta {
                stop_reason: Some("end_turn".into()),
                output_tokens,
            });
            let _ = tx.send(StreamEvent::MessageStop);
        }

        let _ = child.wait();
    });

    Ok(rx)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn build_args(req: &CliRequest, streaming: bool) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        req.prompt.clone(),
        "--output-format".to_string(),
        if streaming {
            "stream-json".to_string()
        } else {
            "json".to_string()
        },
        "--verbose".to_string(),
        "--model".to_string(),
        req.model.clone(),
        "--no-session-persistence".to_string(),
    ];

    if req.dangerously_skip_permissions {
        args.insert(0, "--dangerously-skip-permissions".to_string());
    }

    if let Some(system) = &req.system_prompt {
        args.extend(["--system-prompt".to_string(), system.clone()]);
    }

    args
}

/// Parse CLI output, handling both JSON array (verbose) and single-object formats.
fn parse_cli_output(stdout: &str) -> Result<serde_json::Value> {
    let trimmed = stdout.trim();

    if trimmed.starts_with('[') {
        // Verbose mode: array of events — find the "result" event
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(trimmed).map_err(|e| Error::Terminal(format!(
                "failed to parse CLI JSON array: {e}"
            )))?;

        arr.into_iter()
            .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("result"))
            .ok_or_else(|| Error::Terminal("no result event in CLI output".into()))
    } else {
        // Single JSON object — find the last JSON line
        let json_str = trimmed
            .lines()
            .rev()
            .find(|line| line.starts_with('{'))
            .unwrap_or(trimmed);

        serde_json::from_str(json_str).map_err(|e| Error::Terminal(format!(
            "failed to parse CLI JSON: {e}"
        )))
    }
}

/// Rough token estimate (~4 chars per token).
fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32 / 4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_non_streaming() {
        let req = CliRequest {
            prompt: "hello".into(),
            system_prompt: None,
            model: "claude-sonnet-4-6".into(),
            output_format: OutputFormat::Json,
            dangerously_skip_permissions: false,
        };
        let args = build_args(&req, false);
        assert!(args.contains(&"json".to_string()));
        assert!(!args.contains(&"stream-json".to_string()));
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn build_args_streaming() {
        let req = CliRequest {
            prompt: "hello".into(),
            system_prompt: Some("be helpful".into()),
            model: "claude-opus-4-6".into(),
            output_format: OutputFormat::StreamJson,
            dangerously_skip_permissions: true,
        };
        let args = build_args(&req, true);
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--system-prompt".to_string()));
        assert!(args.contains(&"be helpful".to_string()));
    }

    #[test]
    fn parse_cli_output_single_object() {
        let json = r#"{"type":"result","result":"hello","cost_usd":0.001}"#;
        let parsed = parse_cli_output(json).unwrap();
        assert_eq!(parsed["type"], "result");
        assert_eq!(parsed["result"], "hello");
    }

    #[test]
    fn parse_cli_output_array() {
        let json = r#"[{"type":"system"},{"type":"assistant"},{"type":"result","result":"world"}]"#;
        let parsed = parse_cli_output(json).unwrap();
        assert_eq!(parsed["type"], "result");
        assert_eq!(parsed["result"], "world");
    }

    #[test]
    fn parse_cli_output_no_result_in_array() {
        let json = r#"[{"type":"system"},{"type":"assistant"}]"#;
        let result = parse_cli_output(json);
        assert!(result.is_err());
    }

    #[test]
    fn estimate_tokens_minimum_one() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("hi"), 1);
    }

    #[test]
    fn estimate_tokens_rough_count() {
        let text = "a".repeat(100);
        assert_eq!(estimate_tokens(&text), 25);
    }
}
