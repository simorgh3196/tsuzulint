//! Cache error types.

use thiserror::Error;

/// Errors that can occur in the cache system.
#[derive(Debug, Error)]
pub enum CacheError {
    /// Failed to read cache file.
    #[error("Failed to read cache: {0}")]
    ReadError(String),

    /// Failed to write cache file.
    #[error("Failed to write cache: {0}")]
    WriteError(String),

    /// Cache is corrupted.
    #[error("Corrupted cache: {0}")]
    Corrupted(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

impl CacheError {
    /// Creates a read error.
    pub fn read(message: impl Into<String>) -> Self {
        Self::ReadError(message.into())
    }

    /// Creates a write error.
    pub fn write(message: impl Into<String>) -> Self {
        Self::WriteError(message.into())
    }

    /// Creates a corrupted cache error.
    pub fn corrupted(message: impl Into<String>) -> Self {
        Self::Corrupted(message.into())
    }
}
