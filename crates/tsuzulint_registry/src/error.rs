//! Error types for manifest fetching operations.

use crate::ManifestError;
use crate::security::SecurityError;
use std::string::FromUtf8Error;
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

    /// Security error (SSRF protection).
    #[error("Security error: {0}")]
    SecurityError(#[from] SecurityError),

    /// Secure HTTP fetch error.
    #[error("Secure fetch error: {0}")]
    SecureFetchError(#[from] crate::http_client::SecureFetchError),

    /// Invalid UTF-8 in response body.
    #[error("Invalid UTF-8 in response: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),
}
