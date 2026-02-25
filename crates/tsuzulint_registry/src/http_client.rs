//! Secure HTTP client with SSRF and DNS Rebinding protection.

use std::time::Duration;

use crate::security::SecurityError;
use crate::security::{check_ip, validate_url};
use reqwest::Url;
use thiserror::Error;
use tokio::net::lookup_host;

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
        let mut current_url_str = url.to_string();
        let mut redirect_count = 0;

        loop {
            let parsed_url = Url::parse(&current_url_str)
                .map_err(|e| SecureFetchError::NotFound(format!("Invalid URL: {}", e)))?;

            // 1. Basic URL validation
            validate_url(&parsed_url, self.allow_local)?;

            // 2. Build client with DNS pinning
            let client = self.build_client(&parsed_url).await?;

            // 3. Perform request
            let response = client
                .get(parsed_url.clone())
                .timeout(self.timeout)
                .send()
                .await?;

            // 4. Handle redirects
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .ok_or_else(|| {
                        SecureFetchError::InvalidRedirectUrl(
                            "Redirect response missing Location header".to_string(),
                        )
                    })?;

                let location_str = location.to_str().map_err(|_| {
                    SecureFetchError::InvalidRedirectUrl(
                        "Location header is not valid UTF-8".to_string(),
                    )
                })?;

                let next_url = parsed_url.join(location_str).map_err(|e| {
                    SecureFetchError::InvalidRedirectUrl(format!(
                        "Failed to parse redirect URL: {}",
                        e
                    ))
                })?;

                redirect_count += 1;
                if redirect_count >= self.max_redirects {
                    return Err(SecureFetchError::RedirectLimitExceeded);
                }

                current_url_str = next_url.to_string();
                continue;
            }

            // 5. Handle response status
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(SecureFetchError::NotFound(format!(
                    "Resource not found at {}",
                    current_url_str
                )));
            }

            let response = response.error_for_status().map_err(|e| {
                if let Some(status) = e.status() {
                    SecureFetchError::HttpError(status)
                } else {
                    SecureFetchError::NetworkError(e)
                }
            })?;

            let bytes = response.bytes().await?.to_vec();
            return Ok(bytes);
        }
    }

    async fn build_client(&self, url: &Url) -> Result<reqwest::Client, SecureFetchError> {
        if self.allow_local {
            return reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| SecureFetchError::ClientBuildError(e.to_string()));
        }

        let Some(host) = url.host_str() else {
            return Err(SecureFetchError::NotFound("URL has no host".to_string()));
        };

        let port = url.port_or_known_default().unwrap_or(80);

        // DNS resolution with timeout
        let addrs: Vec<_> = tokio::time::timeout(self.timeout, lookup_host((host, port)))
            .await
            .map_err(|_| SecureFetchError::DnsTimeout)?
            .map_err(|e| SecureFetchError::NotFound(format!("DNS resolution failed: {}", e)))?
            .collect();

        if addrs.is_empty() {
            return Err(SecureFetchError::DnsNoAddress(host.to_string()));
        }

        // Validate all resolved IPs
        for addr in &addrs {
            check_ip(addr.ip())?;
        }

        // Build client with DNS pinning
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &addrs)
            .build()
            .map_err(|e| SecureFetchError::ClientBuildError(e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn builder_default_values() {
        let client = SecureHttpClient::builder().build();

        assert_eq!(client.timeout, Duration::from_secs(10));
        assert!(!client.allow_local);
        assert_eq!(client.max_redirects, 10);
    }

    #[test]
    fn builder_custom_values() {
        let client = SecureHttpClient::builder()
            .timeout(Duration::from_secs(30))
            .allow_local(true)
            .max_redirects(5)
            .build();

        assert_eq!(client.timeout, Duration::from_secs(30));
        assert!(client.allow_local);
        assert_eq!(client.max_redirects, 5);
    }

    #[test]
    fn default_impl_uses_builder_defaults() {
        let client = SecureHttpClient::default();
        let built = SecureHttpClient::builder().build();

        assert_eq!(client.timeout, built.timeout);
        assert_eq!(client.allow_local, built.allow_local);
        assert_eq!(client.max_redirects, built.max_redirects);
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/data"))
            .respond_with(ResponseTemplate::new(200).set_body_string("test content"))
            .mount(&mock_server)
            .await;

        let client = SecureHttpClient::builder().allow_local(true).build();
        let url = format!("{}/data", mock_server.uri());
        let result = client.fetch(&url).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"test content");
    }

    #[tokio::test]
    async fn test_fetch_redirect_loop() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/loop"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", format!("{}/loop", mock_server.uri())),
            )
            .mount(&mock_server)
            .await;

        let client = SecureHttpClient::builder().allow_local(true).build();
        let url = format!("{}/loop", mock_server.uri());
        let result = client.fetch(&url).await;

        assert!(matches!(
            result,
            Err(SecureFetchError::RedirectLimitExceeded)
        ));
    }

    #[tokio::test]
    async fn test_fetch_redirect_success() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", format!("{}/target", mock_server.uri())),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/target"))
            .respond_with(ResponseTemplate::new(200).set_body_string("redirected"))
            .mount(&mock_server)
            .await;

        let client = SecureHttpClient::builder().allow_local(true).build();
        let url = format!("{}/redirect", mock_server.uri());
        let result = client.fetch(&url).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"redirected");
    }

    #[tokio::test]
    async fn test_fetch_local_denied_by_default() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/data"))
            .respond_with(ResponseTemplate::new(200).set_body_string("test"))
            .mount(&mock_server)
            .await;

        let client = SecureHttpClient::builder().build(); // allow_local = false
        let url = format!("{}/data", mock_server.uri());
        let result = client.fetch(&url).await;

        assert!(matches!(result, Err(SecureFetchError::SecurityError(_))));
    }

    #[tokio::test]
    async fn test_fetch_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let client = SecureHttpClient::builder().allow_local(true).build();
        let url = format!("{}/missing", mock_server.uri());
        let result = client.fetch(&url).await;

        assert!(matches!(result, Err(SecureFetchError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_fetch_invalid_utf8_redirect() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/bad-redirect"))
            .respond_with(ResponseTemplate::new(301).append_header("Location", b"\xFF".as_slice()))
            .mount(&mock_server)
            .await;

        let client = SecureHttpClient::builder().allow_local(true).build();
        let url = format!("{}/bad-redirect", mock_server.uri());
        let result = client.fetch(&url).await;

        assert!(matches!(
            result,
            Err(SecureFetchError::InvalidRedirectUrl(_))
        ));
    }
}
