//! Secret redaction for content sent to external APIs.
//!
//! Strips API keys, PEM blocks, Bearer tokens, and other credentials
//! before the planning context is sent over the wire.

use once_cell::sync::Lazy;
use regex::Regex;

/// Placeholder used for redacted values.
const REDACTED: &str = "[REDACTED]";

/// Patterns that match secrets in text content.
static SECRET_PATTERNS: Lazy<Vec<(&str, Regex)>> = Lazy::new(|| {
    vec![
        (
            "env_api_key",
            // ANTHROPIC_API_KEY=sk-ant-... or OPENAI_API_KEY=sk-...
            Regex::new(
                r"(?i)(?:ANTHROPIC_API_KEY|OPENAI_API_KEY|API_KEY|SECRET_KEY|ACCESS_KEY)\s*[=:]\s*\S+",
            )
            .unwrap(),
        ),
        (
            "bearer_token",
            // Authorization: Bearer <token>
            Regex::new(r"(?i)(?:bearer|token)\s+[a-zA-Z0-9\-_\.]{20,}").unwrap(),
        ),
        (
            "generic_secret",
            // password = "...", secret = "...", token = "..."
            Regex::new(
                r#"(?i)(?:password|secret|token|credential|private_key)\s*[=:]\s*["']?[^\s"']{8,}["']?"#,
            )
            .unwrap(),
        ),
        (
            "pem_block",
            // -----BEGIN RSA PRIVATE KEY----- ... -----END RSA PRIVATE KEY-----
            Regex::new(r"-----BEGIN [A-Z ]+-----[\s\S]*?-----END [A-Z ]+-----").unwrap(),
        ),
        (
            "aws_key",
            // AWS access key ID pattern (AKIA...)
            Regex::new(r"(?:AKIA|ASIA)[A-Z0-9]{16}").unwrap(),
        ),
        (
            "sk_ant_key",
            // Anthropic API key pattern
            Regex::new(r"sk-ant-[a-zA-Z0-9\-_]{20,}").unwrap(),
        ),
        (
            "sk_key",
            // OpenAI-style key pattern
            Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(),
        ),
        (
            "connection_string",
            // postgresql://user:pass@host, mongodb://...
            Regex::new(r"(?:postgresql|postgres|mysql|mongodb|redis)://[^\s]+:[^\s@]+@[^\s]+")
                .unwrap(),
        ),
    ]
});

/// Sanitize text by redacting any detected secrets.
///
/// Returns a new string with all matched secret patterns replaced by `[REDACTED]`.
/// Safe to call on any content — returns unchanged text if no secrets are found.
pub fn sanitize_for_api(text: &str) -> String {
    let mut result = text.to_string();

    for (_name, pattern) in SECRET_PATTERNS.iter() {
        result = pattern.replace_all(&result, REDACTED).to_string();
    }

    result
}

/// Check whether text contains any detectable secrets.
///
/// Useful for pre-flight checks before sending content to an API.
pub fn contains_secrets(text: &str) -> bool {
    SECRET_PATTERNS.iter().any(|(_, p)| p.is_match(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_anthropic_key() {
        let input = "ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz";
        let output = sanitize_for_api(input);
        assert_eq!(output, REDACTED);
        assert!(!output.contains("sk-ant"));
    }

    #[test]
    fn sanitize_openai_key() {
        let input = "OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz1234567890";
        let output = sanitize_for_api(input);
        assert!(!output.contains("sk-proj"));
    }

    #[test]
    fn sanitize_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc123";
        let output = sanitize_for_api(input);
        assert!(!output.contains("eyJhbG"));
        assert!(output.contains(REDACTED));
    }

    #[test]
    fn sanitize_pem_block() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIBogIBAAJBALH5\n-----END RSA PRIVATE KEY-----";
        let output = sanitize_for_api(input);
        assert_eq!(output, REDACTED);
    }

    #[test]
    fn sanitize_password_in_config() {
        let input = r#"password = "super_secret_value_123""#;
        let output = sanitize_for_api(input);
        assert!(!output.contains("super_secret"));
    }

    #[test]
    fn sanitize_connection_string() {
        let input = "DATABASE_URL=postgresql://admin:s3cretP@ss@db.example.com:5432/mydb";
        let output = sanitize_for_api(input);
        assert!(!output.contains("s3cretP@ss"));
    }

    #[test]
    fn sanitize_aws_key() {
        let input = "aws_key = AKIAIOSFODNN7EXAMPLE";
        let output = sanitize_for_api(input);
        assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn sanitize_preserves_normal_text() {
        let input = "This is normal text about adding a database table.";
        let output = sanitize_for_api(input);
        assert_eq!(output, input);
    }

    #[test]
    fn sanitize_multiple_secrets() {
        let input = "ANTHROPIC_API_KEY=sk-ant-test123456789012345\n\
                      Also password=\"my_pass_1234567890\"";
        let output = sanitize_for_api(input);
        assert!(!output.contains("sk-ant"));
        assert!(!output.contains("my_pass"));
    }

    #[test]
    fn contains_secrets_positive() {
        assert!(contains_secrets("ANTHROPIC_API_KEY=sk-ant-abcdefghijklmnopqrstuvwxyz"));
        assert!(contains_secrets("Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature"));
    }

    #[test]
    fn contains_secrets_negative() {
        assert!(!contains_secrets("Normal text about architecture"));
        assert!(!contains_secrets("flowchart LR\n  A-->B"));
    }

    #[test]
    fn sanitize_sk_ant_inline() {
        let input = "The key is sk-ant-api03-1234567890abcdefghijklmnop in the config.";
        let output = sanitize_for_api(input);
        assert!(!output.contains("sk-ant"));
    }

    #[test]
    fn sanitize_env_var_with_colon() {
        let input = "API_KEY: my-super-secret-api-key-value";
        let output = sanitize_for_api(input);
        assert!(!output.contains("my-super-secret"));
    }
}
