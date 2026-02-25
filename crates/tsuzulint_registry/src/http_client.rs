//! Secure HTTP client with SSRF and DNS Rebinding protection.

use crate::security::SecurityError;
use thiserror::Error;

/// Error type for secure HTTP fetch operations.
#[derive(Debug, Error)]
pub enum SecureFetchError {
    /// Too many redirects.
    #[error("Too many redirects")]
    RedirectLimitExceeded,

    /// Invalid redirect URL.
    #[error("Invalid redirect URL: {0}")]
    InvalidRedirectUrl(String),

    /// DNS resolution returned no addresses.
    #[error("DNS resolution returned no addresses for host: {0}")]
    DnsNoAddress(String),

    /// DNS resolution timed out.
    #[error("DNS resolution timed out")]
    DnsTimeout,

    /// Network request failed.
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// Security error (SSRF protection).
    #[error("Security error: {0}")]
    SecurityError(#[from] SecurityError),

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// HTTP error status.
    #[error("HTTP error: {0}")]
    HttpError(reqwest::StatusCode),

    /// Failed to build HTTP client.
    #[error("Failed to build HTTP client: {0}")]
    ClientBuildError(String),
}
