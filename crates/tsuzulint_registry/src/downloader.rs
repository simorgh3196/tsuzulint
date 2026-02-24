//! WASM downloader for plugin artifacts.

use crate::hash::HashVerifier;
use crate::manifest::ExternalRuleManifest;
use crate::security::{SecurityError, check_ip, validate_url};
use futures_util::StreamExt;
use reqwest::Url;
use std::time::Duration;
use thiserror::Error;
use tokio::net::lookup_host;

/// Default maximum file size for WASM downloads (50 MB).
pub const DEFAULT_MAX_SIZE: u64 = 50 * 1024 * 1024;

/// Default request timeout (60 seconds).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Error type for WASM download operations.
#[derive(Debug, Error)]
pub enum DownloadError {
    /// Network request failed.
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// File size exceeds the maximum allowed.
    #[error("File too large: {size} bytes exceeds maximum of {max} bytes")]
    TooLarge { size: u64, max: u64 },

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Security error (SSRF protection).
    #[error("Security error: {0}")]
    SecurityError(#[from] SecurityError),

    /// Too many redirects.
    #[error("Too many redirects")]
    RedirectLimitExceeded,

    /// Invalid redirect URL.
    #[error("Invalid redirect URL: {0}")]
    InvalidRedirectUrl(String),

    /// Failed to build HTTP client.
    #[error("Failed to build HTTP client: {0}")]
    ClientBuildError(String),

    /// DNS resolution returned no addresses.
    #[error("DNS resolution returned no addresses for host: {0}")]
    DnsNoAddress(String),
}

/// Result of a successful WASM download.
#[derive(Debug)]
pub struct DownloadResult {
    /// Downloaded WASM binary.
    pub bytes: Vec<u8>,
    /// Computed SHA256 hash of the downloaded bytes (lowercase hex).
    pub computed_hash: String,
}

/// Downloader for WASM artifacts from plugin manifests.
pub struct WasmDownloader {
    client: reqwest::Client,
    max_size: u64,
    timeout: Duration,
    allow_local: bool,
}

impl WasmDownloader {
    /// Create a new WASM downloader with default settings.
    pub fn new() -> Result<Self, DownloadError> {
        Self::create(DEFAULT_MAX_SIZE, DEFAULT_TIMEOUT, false)
    }

    /// Create a new WASM downloader with a custom maximum file size.
    pub fn with_max_size(max_size: u64) -> Result<Self, DownloadError> {
        Self::create(max_size, DEFAULT_TIMEOUT, false)
    }

    /// Create a new WASM downloader with custom settings.
    pub fn with_options(max_size: u64, timeout: Duration) -> Result<Self, DownloadError> {
        Self::create(max_size, timeout, false)
    }

    fn create(max_size: u64, timeout: Duration, allow_local: bool) -> Result<Self, DownloadError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| DownloadError::ClientBuildError(e.to_string()))?;

        Ok(Self {
            client,
            max_size,
            timeout,
            allow_local,
        })
    }

    /// Configure whether to allow downloads from local network addresses.
    ///
    /// By default, downloads from loopback, link-local, and private IP ranges are blocked
    /// to prevent SSRF attacks. Set this to `true` to allow them (e.g. for testing).
    pub fn allow_local(mut self, allow: bool) -> Self {
        self.allow_local = allow;
        self
    }

    /// Download WASM from the manifest's artifact URL.
    ///
    /// This method:
    /// 1. Replaces `{version}` placeholder in the URL with the manifest version
    /// 2. Downloads the WASM binary with streaming (early size limit check)
    /// 3. Computes the SHA256 hash after download
    ///
    /// Note: Hash verification against `manifest.artifacts.sha256` is the caller's responsibility.
    pub async fn download(
        &self,
        manifest: &ExternalRuleManifest,
    ) -> Result<DownloadResult, DownloadError> {
        let url = self.resolve_url(manifest);
        self.download_from_url(&url).await
    }

    /// Resolve the download URL by replacing `{version}` placeholder.
    fn resolve_url(&self, manifest: &ExternalRuleManifest) -> String {
        manifest
            .artifacts
            .wasm
            .replace("{version}", &manifest.rule.version)
    }

    /// Download WASM from a resolved URL using streaming.
    async fn download_from_url(
        &self,
        initial_url_str: &str,
    ) -> Result<DownloadResult, DownloadError> {
        let mut current_url_str = initial_url_str.to_string();
        let mut redirect_count = 0;
        const MAX_REDIRECTS: u32 = 10;

        loop {
            if redirect_count >= MAX_REDIRECTS {
                return Err(DownloadError::RedirectLimitExceeded);
            }

            let url = Url::parse(&current_url_str)
                .map_err(|e| DownloadError::NotFound(format!("Invalid URL: {}", e)))?;

            // 1. Basic URL validation (scheme, string host checks)
            validate_url(&url, self.allow_local)?;

            // 2. DNS Resolution and IP Validation (SSRF protection)
            // Note: validate_url restricts to http/https when allow_local is false,
            // so a None host (e.g., file://) cannot reach this branch.

            let client = if !self.allow_local {
                if let Some(host) = url.host_str() {
                    let port = url.port_or_known_default().unwrap_or(80);
                    // Force a fresh resolution to avoid stale cache or rebinding
                    let addrs: Vec<_> =
                        tokio::time::timeout(self.timeout, lookup_host((host, port)))
                            .await
                            .map_err(|_| {
                                std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    "DNS lookup timed out",
                                )
                            })??
                            .collect();

                    if addrs.is_empty() {
                        return Err(DownloadError::DnsNoAddress(host.to_string()));
                    }

                    // Verify all resolved IPs
                    for addr in &addrs {
                        check_ip(addr.ip())?;
                    }

                    // Pin all validated IPs. This prevents DNS rebinding (TOCTOU) and allows
                    // hyper to use the happy eyeballs algorithm for dual-stack hosts.
                    reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .resolve_to_addrs(host, &addrs)
                        .build()
                        .map_err(|e| DownloadError::ClientBuildError(e.to_string()))?
                } else {
                    // Fallback for schemes without host (shouldn't happen for http/s due to validation)
                    self.client.clone()
                }
            } else {
                // Testing mode: allow local addresses, skip pinning
                self.client.clone()
            };

            // 3. Perform Request (without following redirects)
            let response = client.get(url.clone()).timeout(self.timeout).send().await?;

            // 4. Handle Redirects
            if response.status().is_redirection()
                && let Some(location) = response.headers().get(reqwest::header::LOCATION)
            {
                let location_str = location.to_str().map_err(|_| {
                    DownloadError::InvalidRedirectUrl(
                        "Location header is not valid UTF-8".to_string(),
                    )
                })?;

                // Resolve relative URLs
                let next_url = url.join(location_str).map_err(|e| {
                    DownloadError::InvalidRedirectUrl(format!(
                        "Failed to parse redirect URL: {}",
                        e
                    ))
                })?;

                current_url_str = next_url.to_string();
                redirect_count += 1;
                continue;
            }

            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(DownloadError::NotFound(format!(
                    "WASM file not found at {current_url_str}"
                )));
            }

            // Check HTTP status first to prioritize server errors over size errors
            let response = response.error_for_status()?;

            // Check Content-Length header if available (early rejection)
            let content_length = response.content_length();
            if let Some(len) = content_length
                && len > self.max_size
            {
                return Err(DownloadError::TooLarge {
                    size: len,
                    max: self.max_size,
                });
            }

            // Stream the body while checking size
            let mut stream = response.bytes_stream();
            let mut bytes = if let Some(len) = content_length {
                // Safely convert u64 to usize with fallback for 32-bit platforms
                let capacity = usize::try_from(len)
                    .unwrap_or_else(|_| usize::try_from(self.max_size.min(len)).unwrap_or(0));
                Vec::with_capacity(capacity)
            } else {
                Vec::new()
            };
            let mut total_size: u64 = 0;

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                total_size += chunk.len() as u64;

                // Early rejection if size exceeds limit
                if total_size > self.max_size {
                    return Err(DownloadError::TooLarge {
                        size: total_size,
                        max: self.max_size,
                    });
                }

                bytes.extend_from_slice(&chunk);
            }

            let computed_hash = HashVerifier::compute(&bytes);

            return Ok(DownloadResult {
                bytes,
                computed_hash,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_dummy_manifest(wasm_url: String) -> ExternalRuleManifest {
        ExternalRuleManifest {
            rule: crate::manifest::RuleMetadata {
                name: "test-rule".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                repository: None,
                license: None,
                authors: vec![],
                keywords: vec![],
                fixable: false,
                node_types: vec![],
                isolation_level: crate::manifest::IsolationLevel::Global,
            },
            artifacts: crate::manifest::Artifacts {
                wasm: wasm_url,
                sha256: "ignored_in_download_method".to_string(),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        }
    }

    #[test]
    fn test_version_placeholder_replacement() -> Result<(), DownloadError> {
        let manifest = ExternalRuleManifest {
            rule: crate::manifest::RuleMetadata {
                name: "test-rule".to_string(),
                version: "1.2.3".to_string(),
                description: None,
                repository: None,
                license: None,
                authors: vec![],
                keywords: vec![],
                fixable: false,
                node_types: vec![],
                isolation_level: crate::manifest::IsolationLevel::Global,
            },
            artifacts: crate::manifest::Artifacts {
                wasm: "https://example.com/releases/v{version}/rule.wasm".to_string(),
                sha256: "0".repeat(64),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        };

        let downloader = WasmDownloader::new()?;
        let url = downloader.resolve_url(&manifest);

        assert_eq!(url, "https://example.com/releases/v1.2.3/rule.wasm");
        Ok(())
    }

    #[test]
    fn test_default_max_size_is_50mb() -> Result<(), DownloadError> {
        let downloader = WasmDownloader::new()?;
        assert_eq!(downloader.max_size, 50 * 1024 * 1024);
        Ok(())
    }

    #[test]
    fn test_default_timeout_is_60_seconds() -> Result<(), DownloadError> {
        let downloader = WasmDownloader::new()?;
        assert_eq!(downloader.timeout, Duration::from_secs(60));
        Ok(())
    }

    #[test]
    fn test_custom_options() -> Result<(), DownloadError> {
        let downloader = WasmDownloader::with_options(100 * 1024 * 1024, Duration::from_secs(60))?;
        assert_eq!(downloader.max_size, 100 * 1024 * 1024);
        assert_eq!(downloader.timeout, Duration::from_secs(60));
        Ok(())
    }

    #[tokio::test]
    async fn test_download_success() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        // Mock a valid WASM file (just some random bytes)
        let wasm_content = b"\x00\x61\x73\x6d\x01\x00\x00\x00";
        let expected_hash = HashVerifier::compute(wasm_content);

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm_content.as_slice()))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        // Explicitly allow local for tests
        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&manifest).await?;

        assert_eq!(result.bytes, wasm_content);
        assert_eq!(result.computed_hash, expected_hash);
        Ok(())
    }

    #[tokio::test]
    async fn test_download_not_found() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::NotFound(_)) => Ok(()),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_download_server_error() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::NetworkError(e)) => {
                assert!(e.status() == Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
                Ok(())
            }
            _ => panic!("Expected NetworkError with 500 status"),
        }
    }

    #[tokio::test]
    async fn test_download_too_large_content_length() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let max_size = 10;

        Mock::given(method("GET"))
            .and(path("/large.wasm"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("A".repeat(20)), // wiremock automatically sets Content-Length
            )
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/large.wasm", mock_server.uri()));

        let downloader = WasmDownloader::with_max_size(max_size)?.allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::TooLarge { size, max }) => {
                assert_eq!(size, 20);
                assert_eq!(max, max_size);
                Ok(())
            }
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[tokio::test]
    async fn test_download_too_large_stream() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let max_size = 5;

        Mock::given(method("GET"))
            .and(path("/stream.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_string("A".repeat(10)))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/stream.wasm", mock_server.uri()));

        let downloader = WasmDownloader::with_max_size(max_size)?.allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::TooLarge { size: _, max }) => {
                assert_eq!(max, max_size);
                Ok(())
            }
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[tokio::test]
    async fn test_download_no_content_length() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let wasm_content = b"\x00\x61\x73\x6d\x01\x00\x00\x00";
        let expected_hash = HashVerifier::compute(wasm_content);

        // Serve with chunked transfer encoding (no Content-Length)
        Mock::given(method("GET"))
            .and(path("/chunked.wasm"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(wasm_content.as_slice())
                    .insert_header("Transfer-Encoding", "chunked"),
            )
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/chunked.wasm", mock_server.uri()));

        // Explicitly allow local because wiremock runs on localhost
        let downloader = WasmDownloader::new()?.allow_local(true);

        let result = downloader.download(&manifest).await?;

        assert_eq!(result.bytes, wasm_content);
        assert_eq!(result.computed_hash, expected_hash);
        Ok(())
    }

    #[tokio::test]
    async fn test_download_local_denied_by_default() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        // Default downloader (deny local)
        let downloader = WasmDownloader::new()?;
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::SecurityError(SecurityError::LoopbackDenied(_))) => {
                // Success - access denied
                Ok(())
            }
            res => panic!("Expected SecurityError::LoopbackDenied, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_redirect_success() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let wasm_content = b"\x00\x61\x73\x6d\x01\x00\x00\x00";

        // Redirect from /redirect to /rule.wasm
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(ResponseTemplate::new(301).insert_header(
                "Location",
                format!("{}/rule.wasm", mock_server.uri()).as_str(),
            ))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm_content.as_slice()))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/redirect", mock_server.uri()));

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&manifest).await?;

        assert_eq!(result.bytes, wasm_content);
        Ok(())
    }

    #[tokio::test]
    async fn test_redirect_limit_exceeded() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        // Redirect loop
        Mock::given(method("GET"))
            .and(path("/loop"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", format!("{}/loop", mock_server.uri()).as_str()),
            )
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/loop", mock_server.uri()));

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::RedirectLimitExceeded) => Ok(()),
            res => panic!("Expected RedirectLimitExceeded, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_secure_download_rejects_private_ip_directly() -> Result<(), DownloadError> {
        // Verify that a URL whose hostname resolves only to a private IP is rejected
        // even when the URL string itself looks public.
        // Uses a wiremock server (localhost) with allow_local = false so the DNS
        // resolution step is exercised and LoopbackDenied is returned.
        let mock_server = MockServer::start().await;
        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        let downloader = WasmDownloader::new()?; // allow_local = false by default
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::SecurityError(SecurityError::LoopbackDenied(_))) => Ok(()),
            res => panic!("Expected SecurityError::LoopbackDenied, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_secure_download_builds_client_for_public_ip() -> Result<(), DownloadError> {
        // This test exercises the code path where the IP check *passes* (secure mode),
        // triggering the client construction and `resolve_to_addrs` pinning.
        // We use TEST-NET-1 (192.0.2.1), which is a reserved public block.
        // It passes check_ip() but is not routable/will timeout, allowing us to
        // verify the client build logic without reaching an external server.

        // Use a short timeout to make the test fast
        let downloader =
            WasmDownloader::with_options(DEFAULT_MAX_SIZE, Duration::from_millis(100))?;

        let manifest = create_dummy_manifest("http://192.0.2.1/rule.wasm".to_string());
        let result = downloader.download(&manifest).await;

        // We expect a NetworkError (timeout/unreachable) or IoError, NOT a SecurityError.
        match result {
            Ok(_) => panic!("Expected failure for unreachable IP"),
            Err(DownloadError::SecurityError(e)) => {
                panic!("Should not fail security check: {:?}", e)
            }
            Err(_) => Ok(()), // Any other error means we attempted the connection
        }
    }
}
