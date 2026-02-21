use crate::hash::{HashError, HashVerifier};
use crate::manifest::ExternalRuleManifest;
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;
use thiserror::Error;

const DEFAULT_MAX_SIZE: u64 = 50 * 1024 * 1024; // 50 MB
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    #[error("File too large: {size} bytes (max: {max} bytes)")]
    TooLarge { size: u64, max: u64 },
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("Hash verification failed: {0}")]
    HashError(#[from] HashError),
    #[error("{0}")]
    NotFound(String),
    #[error("Security error: {0}")]
    SecurityError(#[from] SecurityError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Loopback address denied: {0}")]
    LoopbackDenied(String),
    #[error("Invalid URL scheme: {0}")]
    InvalidScheme(String),
}

#[derive(Debug)]
pub struct DownloadResult {
    pub bytes: Vec<u8>,
    pub computed_hash: String,
}

pub struct WasmDownloader {
    client: Client,
    max_size: u64,
    timeout: Duration,
    allow_local: bool,
}

impl Default for WasmDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmDownloader {
    /// Create a new WASM downloader with default settings.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            max_size: DEFAULT_MAX_SIZE,
            timeout: DEFAULT_TIMEOUT,
            allow_local: false,
        }
    }

    /// Create a new WASM downloader with a custom maximum file size.
    pub fn with_max_size(max_size: u64) -> Self {
        Self {
            client: reqwest::Client::new(),
            max_size,
            timeout: DEFAULT_TIMEOUT,
            allow_local: false,
        }
    }

    /// Create a new WASM downloader with custom settings.
    pub fn with_options(max_size: u64, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            max_size,
            timeout,
            allow_local: false,
        }
    }

    /// Allow downloading from local addresses (e.g. localhost, private IPs).
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

    /// Validate the URL before downloading.
    fn validate_url(&self, url_str: &str) -> Result<(), DownloadError> {
        let url = reqwest::Url::parse(url_str).map_err(|e| {
            // reqwest::Error doesn't expose a clean constructor for url::ParseError
            // so we create a generic network error message
            // or we could add a UrlParseError variant to DownloadError
            DownloadError::NotFound(format!("Invalid URL: {e}"))
        })?;

        // Check scheme
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(SecurityError::InvalidScheme(url.scheme().to_string()).into());
        }

        // Check for loopback/private addresses unless explicitly allowed
        if !self.allow_local {
            if let Some(host_str) = url.host_str()
                && (host_str == "localhost"
                    || host_str == "127.0.0.1"
                    || host_str == "::1"
                    || host_str.starts_with("192.168.")
                    || host_str.starts_with("10."))
            {
                return Err(SecurityError::LoopbackDenied(host_str.to_string()).into());
            }
        }

        Ok(())
    }

    /// Download WASM from a resolved URL using streaming.
    async fn download_from_url(&self, url: &str) -> Result<DownloadResult, DownloadError> {
        self.validate_url(url)?;

        let response = self.client.get(url).timeout(self.timeout).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(DownloadError::NotFound(format!(
                "WASM file not found at {url}"
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
            Vec::with_capacity(len as usize)
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
    fn test_version_placeholder_replacement() {
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

        let downloader = WasmDownloader::new();
        let url = downloader.resolve_url(&manifest);

        assert_eq!(url, "https://example.com/releases/v1.2.3/rule.wasm");
    }

    #[test]
    fn test_default_max_size_is_50mb() {
        let downloader = WasmDownloader::new();
        assert_eq!(downloader.max_size, 50 * 1024 * 1024);
    }

    #[test]
    fn test_default_timeout_is_60_seconds() {
        let downloader = WasmDownloader::new();
        assert_eq!(downloader.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_custom_options() {
        let downloader = WasmDownloader::with_options(100 * 1024 * 1024, Duration::from_secs(60));
        assert_eq!(downloader.max_size, 100 * 1024 * 1024);
        assert_eq!(downloader.timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_download_success() {
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
        let downloader = WasmDownloader::new().allow_local(true);
        let result = downloader
            .download(&manifest)
            .await
            .expect("Download failed");

        assert_eq!(result.bytes, wasm_content);
        assert_eq!(result.computed_hash, expected_hash);
    }

    #[tokio::test]
    async fn test_download_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        let downloader = WasmDownloader::new().allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::NotFound(_)) => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_download_server_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        let downloader = WasmDownloader::new().allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::NetworkError(e)) => {
                assert!(e.status() == Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR))
            }
            _ => panic!("Expected NetworkError with 500 status"),
        }
    }

    #[tokio::test]
    async fn test_download_too_large_content_length() {
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

        let downloader = WasmDownloader::with_max_size(max_size).allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::TooLarge { size, max }) => {
                assert_eq!(size, 20);
                assert_eq!(max, max_size);
            }
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[tokio::test]
    async fn test_download_too_large_stream() {
        let mock_server = MockServer::start().await;
        let max_size = 5;

        Mock::given(method("GET"))
            .and(path("/stream.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_string("A".repeat(10)))
            .mount(&mock_server)
            .await;

        let manifest = create_dummy_manifest(format!("{}/stream.wasm", mock_server.uri()));

        let downloader = WasmDownloader::with_max_size(max_size).allow_local(true);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::TooLarge { size: _, max }) => {
                assert_eq!(max, max_size);
            }
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[tokio::test]
    async fn test_download_no_content_length() {
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
        let downloader = WasmDownloader::new().allow_local(true);

        let result = downloader
            .download(&manifest)
            .await
            .expect("Download failed with no Content-Length");

        assert_eq!(result.bytes, wasm_content);
        assert_eq!(result.computed_hash, expected_hash);
    }

    #[tokio::test]
    async fn test_download_local_denied_by_default() {
        let mock_server = MockServer::start().await;

        let manifest = create_dummy_manifest(format!("{}/rule.wasm", mock_server.uri()));

        // Default downloader (deny local)
        let downloader = WasmDownloader::new();
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::SecurityError(SecurityError::LoopbackDenied(_))) => {
                // Success - access denied
            }
            res => panic!("Expected SecurityError::LoopbackDenied, got {:?}", res),
        }
    }
}
