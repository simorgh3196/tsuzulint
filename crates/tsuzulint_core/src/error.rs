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
    #[diagnostic(
        code(tsuzulint::file),
        help("Ensure the file exists and has the correct permissions.")
    )]
    File(String),

    /// Parse error.
    #[error("Parse error: {0}")]
    #[diagnostic(code(tsuzulint::parse), help("Check the file for syntax errors."))]
    Parse(String),

    /// Plugin error.
    #[error("Plugin error: {0}")]
    #[diagnostic(
        code(tsuzulint::plugin),
        help("Verify the plugin is installed and compatible.")
    )]
    Plugin(#[from] tsuzulint_plugin::PluginError),

    /// Cache error.
    #[error("Cache error: {0}")]
    #[diagnostic(code(tsuzulint::cache), help("Try clearing the cache and re-running."))]
    Cache(#[from] tsuzulint_cache::CacheError),

    /// I/O error.
    #[error("I/O error: {0}")]
    #[diagnostic(
        code(tsuzulint::io),
        help("Check file system permissions and disk space.")
    )]
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
