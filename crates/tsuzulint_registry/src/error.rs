//! Error types for manifest fetching operations.

use crate::ManifestError;
use thiserror::Error;

/// Error type for manifest fetch operations.
#[derive(Debug, Error)]
pub enum FetchError {
    /// Network request failed.
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Manifest parsing or validation failed.
    #[error("Invalid manifest: {0}")]
    InvalidManifest(#[from] ManifestError),

    /// File system I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}
