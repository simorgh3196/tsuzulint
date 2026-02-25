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

use std::time::Duration;

/// Default timeout for HTTP requests.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default maximum number of redirects.
pub const DEFAULT_MAX_REDIRECTS: u32 = 10;

/// Secure HTTP client with SSRF and DNS Rebinding protection.
#[derive(Debug, Clone)]
pub struct SecureHttpClient {
    timeout: Duration,
    allow_local: bool,
    max_redirects: u32,
}

/// Builder for SecureHttpClient.
#[derive(Debug)]
pub struct SecureHttpClientBuilder {
    timeout: Duration,
    allow_local: bool,
    max_redirects: u32,
}

impl Default for SecureHttpClient {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl SecureHttpClient {
    /// Create a new builder for SecureHttpClient.
    pub fn builder() -> SecureHttpClientBuilder {
        SecureHttpClientBuilder {
            timeout: DEFAULT_TIMEOUT,
            allow_local: false,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }

    /// Fetch content from URL with SSRF/DNS Rebinding protection.
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>, SecureFetchError> {
        // Implementation in next task
        todo!()
    }
}

impl SecureHttpClientBuilder {
    /// Set timeout for HTTP requests.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Allow fetching from local network addresses.
    pub fn allow_local(mut self, allow: bool) -> Self {
        self.allow_local = allow;
        self
    }

    /// Set maximum number of redirects.
    pub fn max_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    /// Build the SecureHttpClient.
    pub fn build(self) -> SecureHttpClient {
        SecureHttpClient {
            timeout: self.timeout,
            allow_local: self.allow_local,
            max_redirects: self.max_redirects,
        }
    }
}
