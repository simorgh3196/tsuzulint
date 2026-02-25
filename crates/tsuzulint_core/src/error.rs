//! Linter error types.

use miette::Diagnostic;
use thiserror::Error;

/// Errors that can occur during linting.
#[derive(Debug, Error, Diagnostic)]
pub enum LinterError {
    /// Configuration error.
    #[error("Configuration error: {0}")]
    #[diagnostic(
        code(tsuzulint::config),
        help("Check your configuration file syntax and structure.")
    )]
    Config(String),

    /// File I/O error.
    #[error("File error: {0}")]
    #[diagnostic(code(tsuzulint::file))]
    File(String),

    /// Parse error.
    #[error("Parse error: {0}")]
    #[diagnostic(code(tsuzulint::parse))]
    Parse(String),

    /// Plugin error.
    #[error("Plugin error: {0}")]
    #[diagnostic(code(tsuzulint::plugin))]
    Plugin(#[from] tsuzulint_plugin::PluginError),

    /// Cache error.
    #[error("Cache error: {0}")]
    #[diagnostic(code(tsuzulint::cache))]
    Cache(#[from] tsuzulint_cache::CacheError),

    /// I/O error.
    #[error("I/O error: {0}")]
    #[diagnostic(code(tsuzulint::io))]
    Io(#[from] std::io::Error),

    /// Internal error.
    #[error("Internal error: {0}")]
    #[diagnostic(
        code(tsuzulint::internal),
        help("This is likely a bug in TsuzuLint. Please report it.")
    )]
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
