//! Anthropic Messages API client.
//!
//! This module is only compiled when the `api` feature is enabled.
//! It provides a blocking HTTP client that calls the Anthropic Messages API
//! with forced tool use for structured plan output.

use tracing::{debug, info};

use crate::error::{Error, Result};

use super::types::{LlmRequest, LlmResponse};

/// Anthropic API base URL.
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";

/// API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Blocking Anthropic API client.
///
/// Reads `ANTHROPIC_API_KEY` from the environment at construction time.
/// Fails fast with a clear error if the key is missing.
#[derive(Debug, Clone)]
pub struct AnthropicClient {
    api_key: String,
    client: reqwest::blocking::Client,
}

impl AnthropicClient {
    /// Create a new client, reading the API key from `ANTHROPIC_API_KEY` env var.
    ///
    /// Returns `Error::Config` if the key is not set or empty.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            Error::Config(
                "ANTHROPIC_API_KEY not set. \
                 Set it in your environment to use `plan --call-api`.\n\
                 Example: export ANTHROPIC_API_KEY=sk-ant-..."
                    .to_string(),
            )
        })?;

        if api_key.is_empty() {
            return Err(Error::Config(
                "ANTHROPIC_API_KEY is set but empty".to_string(),
            ));
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Config(format!("Failed to create HTTP client: {e}")))?;

        info!("Anthropic client initialized");
        Ok(Self { api_key, client })
    }

    /// Create a client with an explicit API key (useful for testing).
    pub fn new(api_key: String) -> Result<Self> {
        if api_key.is_empty() {
            return Err(Error::Config("API key cannot be empty".to_string()));
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Config(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self { api_key, client })
    }

    /// Send a request to the Anthropic Messages API.
    ///
    /// Constructs the full JSON body including system prompt, messages,
    /// tools, and tool_choice, then sends a blocking POST request.
    pub fn call(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let body = serde_json::json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "messages": request.messages,
            "tools": request.tools,
            "tool_choice": request.tool_choice,
        });

        debug!(
            model = request.model,
            max_tokens = request.max_tokens,
            tools = request.tools.len(),
            "Sending request to Anthropic API"
        );

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| Error::Api {
                status: None,
                message: format!("Request failed: {e}"),
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().unwrap_or_else(|_| "no body".to_string());
            return Err(Error::Api {
                status: Some(status.as_u16()),
                message: format!("API returned {status}: {error_body}"),
            });
        }

        let llm_response: LlmResponse = response.json().map_err(|e| Error::Api {
            status: Some(status.as_u16()),
            message: format!("Failed to parse API response: {e}"),
        })?;

        info!(
            model = llm_response.model,
            input_tokens = llm_response.usage.input_tokens,
            output_tokens = llm_response.usage.output_tokens,
            stop_reason = ?llm_response.stop_reason,
            "Received API response"
        );

        Ok(llm_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_fails_without_key() {
        // Temporarily ensure the key is not set
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = AnthropicClient::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn new_rejects_empty_key() {
        let result = AnthropicClient::new(String::new());
        assert!(result.is_err());
    }

    #[test]
    fn new_accepts_valid_key() {
        let client = AnthropicClient::new("sk-ant-test-key-12345".to_string());
        assert!(client.is_ok());
    }
}
