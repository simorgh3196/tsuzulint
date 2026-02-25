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
}

impl From<crate::http_client::SecureFetchError> for FetchError {
    fn from(e: crate::http_client::SecureFetchError) -> Self {
        match e {
            crate::http_client::SecureFetchError::NotFound(msg) => FetchError::NotFound(msg),
            crate::http_client::SecureFetchError::NetworkError(e) => FetchError::NetworkError(e),
            crate::http_client::SecureFetchError::SecurityError(e) => FetchError::SecurityError(e),
            crate::http_client::SecureFetchError::DnsNoAddress(host) => {
                FetchError::NotFound(format!("DNS resolution failed for host: {}", host))
            }
            crate::http_client::SecureFetchError::DnsTimeout => {
                FetchError::NotFound("DNS resolution timed out".to_string())
            }
            crate::http_client::SecureFetchError::RedirectLimitExceeded => {
                FetchError::NotFound("Too many redirects".to_string())
            }
            crate::http_client::SecureFetchError::InvalidRedirectUrl(msg) => {
                FetchError::NotFound(format!("Invalid redirect: {}", msg))
            }
            crate::http_client::SecureFetchError::HttpError(status) => {
                FetchError::NotFound(format!("HTTP error: {}", status))
            }
            crate::http_client::SecureFetchError::ClientBuildError(msg) => {
                FetchError::NotFound(format!("Client build error: {}", msg))
            }
        }
    }
}
