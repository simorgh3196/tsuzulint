//! Error types for manifest fetching operations.

use crate::ManifestError;
use crate::security::SecurityError;
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

    /// DNS resolution returned no addresses.
    #[error("DNS resolution returned no addresses for host: {0}")]
    DnsNoAddress(String),

    /// Too many redirects.
    #[error("Too many redirects")]
    RedirectLimitExceeded,

    /// Invalid redirect URL.
    #[error("Invalid redirect URL: {0}")]
    InvalidRedirectUrl(String),
}
