//! Linter error types.

use thiserror::Error;

/// Errors that can occur during linting.
#[derive(Debug, Error)]
pub enum LinterError {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// File I/O error.
    #[error("File error: {0}")]
    File(String),

    /// Parse error.
    #[error("Parse error: {0}")]
    Parse(String),

    /// Plugin error.
    #[error("Plugin error: {0}")]
    Plugin(#[from] tsuzulint_plugin::PluginError),

    /// Cache error.
    #[error("Cache error: {0}")]
    Cache(#[from] tsuzulint_cache::CacheError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl LinterError {
    /// Creates a configuration error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// Creates a file error.
    pub fn file(message: impl Into<String>) -> Self {
        Self::File(message.into())
    }

    /// Creates a parse error.
    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }
}
