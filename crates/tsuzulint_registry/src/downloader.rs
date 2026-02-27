//! WASM downloader for plugin artifacts.

use crate::http_client::{SecureFetchError, SecureHttpClient};
use crate::manifest::HashVerifier;
use std::time::Duration;
use thiserror::Error;

/// Default maximum file size for WASM downloads (50 MB).
pub const DEFAULT_MAX_SIZE: u64 = 50 * 1024 * 1024;

/// Default request timeout (60 seconds).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Error type for WASM download operations.
#[derive(Debug, Error)]
pub enum DownloadError {
    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// File size exceeds the maximum allowed.
    #[error("File too large: {size} bytes exceeds maximum of {max} bytes")]
    TooLarge { size: u64, max: u64 },

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Secure HTTP fetch error.
    #[error("Secure fetch error: {0}")]
    SecureHttp(#[from] SecureFetchError),
}

/// Result of a successful WASM download.
#[derive(Debug)]
pub struct DownloadResult {
    /// Downloaded WASM binary.
    pub bytes: Vec<u8>,
    /// Computed SHA256 hash of the downloaded bytes (lowercase hex).
    pub computed_hash: String,
}

pub struct WasmDownloader {
    http_client: SecureHttpClient,
    max_size: u64,
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
        })
    }

    /// Configure whether to allow downloads from local network addresses.
    ///
    /// By default, downloads from loopback, link-local, and private IP ranges are blocked
    /// to prevent SSRF attacks. Set this to `true` to allow them (e.g. for testing).
    pub fn allow_local(mut self, allow: bool) -> Self {
        self.http_client = self.http_client.with_allow_local(allow);
        self
    }

    /// Download WASM from a resolved URL.
    pub async fn download(&self, url: &str) -> Result<DownloadResult, DownloadError> {
        let bytes = self
            .http_client
            .fetch_with_size_limit(url, self.max_size)
            .await
            .map_err(|e| match e {
                SecureFetchError::ResponseTooLarge { size, max } => {
                    DownloadError::TooLarge { size, max }
                }
                other => DownloadError::SecureHttp(other),
            })?;

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
    use reqwest::StatusCode;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

        let url = format!("{}/rule.wasm", mock_server.uri());

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&url).await?;

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

        let url = format!("{}/rule.wasm", mock_server.uri());

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&url).await;

        match result {
            Err(DownloadError::SecureHttp(SecureFetchError::NotFound(_))) => Ok(()),
            _ => panic!("Expected SecureHttp::NotFound"),
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

        let url = format!("{}/rule.wasm", mock_server.uri());

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&url).await;

        match result {
            Err(DownloadError::SecureHttp(SecureFetchError::HttpError(status))) => {
                assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
                Ok(())
            }
            _ => panic!("Expected SecureHttp::HttpError with 500 status"),
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

        let url = format!("{}/large.wasm", mock_server.uri());

        let downloader = WasmDownloader::with_max_size(max_size)?.allow_local(true);
        let result = downloader.download(&url).await;

        match result {
            Err(DownloadError::TooLarge { size, max }) => {
                assert!(
                    size > max,
                    "size should exceed max (streaming: size depends on chunk boundaries)"
                );
                assert_eq!(max, max_size);
                Ok(())
            }
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[tokio::test]
    async fn test_download_too_large_buffered() -> Result<(), DownloadError> {
        // Tests streaming size limit enforcement (Issue #200)
        let mock_server = MockServer::start().await;
        let max_size = 5;

        Mock::given(method("GET"))
            .and(path("/stream.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_string("A".repeat(10)))
            .mount(&mock_server)
            .await;

        let url = format!("{}/stream.wasm", mock_server.uri());

        let downloader = WasmDownloader::with_max_size(max_size)?.allow_local(true);
        let result = downloader.download(&url).await;

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

        let url = format!("{}/chunked.wasm", mock_server.uri());

        let downloader = WasmDownloader::new()?.allow_local(true);

        let result = downloader.download(&url).await?;

        assert_eq!(result.bytes, wasm_content);
        assert_eq!(result.computed_hash, expected_hash);
        Ok(())
    }

    #[tokio::test]
    async fn test_download_local_denied_by_default() -> Result<(), DownloadError> {
        let mock_server = MockServer::start().await;

        let url = format!("{}/rule.wasm", mock_server.uri());

        let downloader = WasmDownloader::new()?;
        let result = downloader.download(&url).await;

        use crate::security::SecurityError;
        match result {
            Err(DownloadError::SecureHttp(SecureFetchError::SecurityError(
                SecurityError::LoopbackDenied(_),
            ))) => Ok(()),
            res => panic!(
                "Expected SecureHttp::SecurityError::LoopbackDenied, got {:?}",
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

        let url = format!("{}/redirect", mock_server.uri());

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&url).await?;

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

        let url = format!("{}/loop", mock_server.uri());

        let downloader = WasmDownloader::new()?.allow_local(true);
        let result = downloader.download(&url).await;

        match result {
            Err(DownloadError::SecureHttp(SecureFetchError::RedirectLimitExceeded)) => Ok(()),
            res => panic!("Expected SecureHttp::RedirectLimitExceeded, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_secure_download_rejects_private_ip_directly() -> Result<(), DownloadError> {
        use crate::security::SecurityError;

        let mock_server = MockServer::start().await;
        let url = format!("{}/rule.wasm", mock_server.uri());

        let downloader = WasmDownloader::new()?;
        let result = downloader.download(&url).await;

        match result {
            Err(DownloadError::SecureHttp(SecureFetchError::SecurityError(
                SecurityError::LoopbackDenied(_),
            ))) => Ok(()),
            res => panic!(
                "Expected SecureHttp::SecurityError::LoopbackDenied, got {:?}",
                res
            ),
        }
    }

    #[tokio::test]
    async fn test_secure_download_builds_client_for_public_ip() -> Result<(), DownloadError> {
        let downloader =
            WasmDownloader::with_options(DEFAULT_MAX_SIZE, Duration::from_millis(100))?;

        let url = "http://192.0.2.1/rule.wasm".to_string();
        let result = downloader.download(&url).await;

        match result {
            Ok(_) => panic!("Expected failure for unreachable IP"),
            Err(DownloadError::SecureHttp(SecureFetchError::SecurityError(e))) => {
                panic!("Should not fail security check: {:?}", e)
            }
            Err(_) => Ok(()),
        }
    }
}
