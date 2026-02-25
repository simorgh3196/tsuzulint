//! Error types for manifest fetching operations.

use crate::ManifestError;
use std::string::FromUtf8Error;
use thiserror::Error;

/// Error type for manifest fetch operations.
#[derive(Debug, Error)]
pub enum FetchError {
    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Manifest parsing or validation failed.
    #[error("Invalid manifest: {0}")]
    InvalidManifest(#[from] ManifestError),

    /// File system I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Secure HTTP fetch error.
    #[error("Secure fetch error: {0}")]
    SecureFetchError(#[from] crate::http_client::SecureFetchError),

    /// Invalid UTF-8 in response body.
    #[error("Invalid UTF-8 in response: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),
}
