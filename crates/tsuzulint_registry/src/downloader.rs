//! WASM downloader for plugin artifacts.

use crate::hash::HashVerifier;
use crate::http_client::SecureFetchError;
use crate::http_client::SecureHttpClient;
use crate::manifest::ExternalRuleManifest;
use crate::security::SecurityError;
use std::time::Duration;
use thiserror::Error;

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

    /// Secure fetch error.
    #[error("Fetch error: {0}")]
    FetchError(#[from] SecureFetchError),
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
    http_client: SecureHttpClient,
    max_size: u64,
    timeout: Duration,
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
        let http_client = SecureHttpClient::builder()
            .timeout(timeout)
            .allow_local(allow_local)
            .build();

        Ok(Self {
            http_client,
            max_size,
            timeout,
        })
    }

    /// Configure whether to allow downloads from local network addresses.
    ///
    /// By default, downloads from loopback, link-local, and private IP ranges are blocked
    /// to prevent SSRF attacks. Set this to `true` to allow them (e.g. for testing).
    pub fn allow_local(self, allow: bool) -> Self {
        Self {
            http_client: SecureHttpClient::builder()
                .timeout(self.timeout)
                .allow_local(allow)
                .build(),
            max_size: self.max_size,
            timeout: self.timeout,
        }
    }

    /// Download WASM from the manifest's artifact URL.
    ///
    /// This method:
    /// 1. Replaces `{version}` placeholder in the URL with the manifest version
    /// 2. Downloads the WASM binary with size limit check
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

    /// Download WASM from a resolved URL.
    async fn download_from_url(
        &self,
        initial_url_str: &str,
    ) -> Result<DownloadResult, DownloadError> {
        let bytes = self.http_client.fetch(initial_url_str).await?;

        if bytes.len() as u64 > self.max_size {
            return Err(DownloadError::TooLarge {
                size: bytes.len() as u64,
                max: self.max_size,
            });
        }

        let computed_hash = HashVerifier::compute(&bytes);

        Ok(DownloadResult {
            bytes,
            computed_hash,
        })
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
    fn test_custom_options() -> Result<(), DownloadError> {
        let downloader = WasmDownloader::with_options(100 * 1024 * 1024, Duration::from_secs(60))?;
        assert_eq!(downloader.max_size, 100 * 1024 * 1024);
        Ok(())
    }

    #[tokio::test]
    async fn test_download_success() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        let wasm_content = b"\x00\x61\x73\x6d\x01\x00\x00\x00";
        let expected_hash = HashVerifier::compute(wasm_content);

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm_content.as_slice()))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

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
            Err(DownloadError::FetchError(SecureFetchError::NotFound(_))) => Ok(()),
            _ => panic!("Expected FetchError::NotFound"),
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
            Err(DownloadError::FetchError(SecureFetchError::HttpError(status))) => {
                assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
                Ok(())
            }
            _ => panic!("Expected FetchError::HttpError with 500 status"),
        }
    }

    #[tokio::test]
    async fn test_download_too_large_content_length() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let max_size = 10;

        Mock::given(method("GET"))
            .and(path("/large.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_string("A".repeat(20)))
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

        let downloader = WasmDownloader::new()?;
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::FetchError(SecureFetchError::SecurityError(
                SecurityError::LoopbackDenied(_),
            ))) => Ok(()),
            res => panic!(
                "Expected FetchError::SecurityError::LoopbackDenied, got {:?}",
                res
            ),
        }
    }

    #[tokio::test]
    async fn test_redirect_success() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let wasm_content = b"\x00\x61\x73\x6d\x01\x00\x00\x00";

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
            Err(DownloadError::FetchError(SecureFetchError::RedirectLimitExceeded)) => Ok(()),
            res => panic!("Expected FetchError::RedirectLimitExceeded, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_secure_download_rejects_private_ip_directly() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;
        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        let downloader = WasmDownloader::new()?;
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::FetchError(SecureFetchError::SecurityError(
                SecurityError::LoopbackDenied(_),
            ))) => Ok(()),
            res => panic!(
                "Expected FetchError::SecurityError::LoopbackDenied, got {:?}",
                res
            ),
        }
    }

    #[tokio::test]
    async fn test_secure_download_builds_client_for_public_ip() -> Result<(), DownloadError> {
        let downloader =
            WasmDownloader::with_options(DEFAULT_MAX_SIZE, Duration::from_millis(100))?;

        let manifest = create_dummy_manifest("http://192.0.2.1/rule.wasm".to_string());
        let result = downloader.download(&manifest).await;

        match result {
            Ok(_) => panic!("Expected failure for unreachable IP"),
            Err(DownloadError::FetchError(SecureFetchError::SecurityError(e))) => {
                panic!("Should not fail security check: {:?}", e)
            }
            Err(_) => Ok(()),
        }
    }
}
