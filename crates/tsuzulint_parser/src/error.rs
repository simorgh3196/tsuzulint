//! Parse error types.

use thiserror::Error;

/// Errors that can occur during parsing.
#[derive(Debug, Error)]
pub enum ParseError {
    /// The source text is invalid.
    #[error("Invalid source: {message}")]
    InvalidSource {
        /// Error message.
        message: String,
        /// Byte offset where the error occurred.
        offset: Option<usize>,
    },

    /// The parser encountered an unsupported feature.
    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    /// An internal parser error occurred.
    #[error("Internal parser error: {0}")]
    Internal(String),
}

impl ParseError {
    /// Creates a new invalid source error.
    pub fn invalid_source(message: impl Into<String>) -> Self {
        Self::InvalidSource {
            message: message.into(),
            offset: None,
        }
    }

    /// Creates a new invalid source error with offset.
    pub fn invalid_source_at(message: impl Into<String>, offset: usize) -> Self {
        Self::InvalidSource {
            message: message.into(),
            offset: Some(offset),
        }
    }

    /// Creates a new unsupported feature error.
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::Unsupported(feature.into())
    }

    /// Creates a new internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}
