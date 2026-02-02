//! Plugin error types.

use thiserror::Error;

/// Errors that can occur in the plugin system.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Failed to load the WASM plugin.
    #[error("Failed to load plugin: {0}")]
    LoadError(String),

    /// Failed to call a plugin function.
    #[error("Plugin call failed: {0}")]
    CallError(String),

    /// Invalid plugin manifest.
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    /// Plugin not found.
    #[error("Plugin not found: {0}")]
    NotFound(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl PluginError {
    /// Creates a load error.
    pub fn load(message: impl Into<String>) -> Self {
        Self::LoadError(message.into())
    }

    /// Creates a call error.
    pub fn call(message: impl Into<String>) -> Self {
        Self::CallError(message.into())
    }

    /// Creates an invalid manifest error.
    pub fn invalid_manifest(message: impl Into<String>) -> Self {
        Self::InvalidManifest(message.into())
    }

    /// Creates a not found error.
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound(name.into())
    }
}
