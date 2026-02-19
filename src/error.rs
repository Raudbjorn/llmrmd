//! Unified error types for llmermaid.
//!
//! All errors are returned as values (Result<T, Error>), never panicked.

use std::path::PathBuf;

/// Central error type for llmermaid operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("JSON parse error at {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("Path does not exist or is not a directory: {0}")]
    InvalidRoot(PathBuf),

    #[error("Index artifacts not found at {0}. Run `llmermaid index` first.")]
    NoIndex(PathBuf),

    #[error("No files found in {0}")]
    EmptyRepo(PathBuf),

    #[error("Empty change description")]
    EmptyDescription,

    #[error("Terminal error: {0}")]
    Terminal(String),

    #[error("{0}")]
    Config(String),

    #[error("API error (status {status:?}): {message}")]
    Api {
        status: Option<u16>,
        message: String,
    },

    #[error("Plan validation failed: {0}")]
    PlanValidation(String),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Helper to create an IO error with path context.
pub fn io_err(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_io() {
        let err = io_err(
            "/tmp/test.txt",
            std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        );
        let msg = err.to_string();
        assert!(msg.contains("/tmp/test.txt"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn error_display_invalid_root() {
        let err = Error::InvalidRoot(PathBuf::from("/nonexistent"));
        assert!(err.to_string().contains("/nonexistent"));
    }

    #[test]
    fn error_display_no_index() {
        let err = Error::NoIndex(PathBuf::from("/repo/.claude"));
        let msg = err.to_string();
        assert!(msg.contains("/repo/.claude"));
        assert!(msg.contains("llmermaid index"));
    }

    #[test]
    fn error_display_empty_description() {
        let err = Error::EmptyDescription;
        assert_eq!(err.to_string(), "Empty change description");
    }

    #[test]
    fn error_display_terminal() {
        let err = Error::Terminal("raw mode failed".to_string());
        assert!(err.to_string().contains("raw mode failed"));
    }

    #[test]
    fn error_display_config() {
        let err = Error::Config("bad config".to_string());
        assert_eq!(err.to_string(), "bad config");
    }

    #[test]
    fn error_display_api_with_status() {
        let err = Error::Api {
            status: Some(429),
            message: "rate limited".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("429"));
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn error_display_api_without_status() {
        let err = Error::Api {
            status: None,
            message: "connection refused".to_string(),
        };
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn error_display_plan_validation() {
        let err = Error::PlanValidation("unknown file referenced".to_string());
        assert!(err.to_string().contains("unknown file referenced"));
    }

    #[test]
    fn io_err_helper() {
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = io_err("/some/path", source);
        match err {
            Error::Io { path, .. } => assert_eq!(path, PathBuf::from("/some/path")),
            _ => panic!("Expected Error::Io"),
        }
    }
}
