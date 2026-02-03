//! WASM downloader for plugin artifacts.

use crate::hash::HashVerifier;
use crate::manifest::ExternalRuleManifest;
use futures_util::StreamExt;
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
        }
    }

    /// Create a new WASM downloader with a custom maximum file size.
    pub fn with_max_size(max_size: u64) -> Self {
        Self {
            client: reqwest::Client::new(),
            max_size,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a new WASM downloader with custom settings.
    pub fn with_options(max_size: u64, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            max_size,
            timeout,
        }
    }

    /// Download WASM from the manifest's artifact URL.
    ///
    /// This method:
    /// 1. Replaces `{version}` placeholder in the URL with the manifest version
    /// 2. Downloads the WASM binary with streaming (early size limit check)
    /// 3. Computes the SHA256 hash during download
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
    async fn download_from_url(&self, url: &str) -> Result<DownloadResult, DownloadError> {
        let response = self.client.get(url).timeout(self.timeout).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(DownloadError::NotFound(format!(
                "WASM file not found at {url}"
            )));
        }

        // Check HTTP status first to prioritize server errors over size errors
        let response = response.error_for_status()?;

        // Check Content-Length header if available (early rejection)
        if let Some(content_length) = response.content_length()
            && content_length > self.max_size
        {
            return Err(DownloadError::TooLarge {
                size: content_length,
                max: self.max_size,
            });
        }

        // Stream the body while checking size
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
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

        let manifest = ExternalRuleManifest {
            rule: crate::manifest::RuleMetadata {
                name: "test-rule".to_string(),
                version: "1.0.0".to_string(),
                // Fill required fields with defaults
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
                wasm: format!("{}/rule.wasm", mock_server.uri()),
                sha256: "ignored_in_download_method".to_string(),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        };

        let downloader = WasmDownloader::new();
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

        let manifest = ExternalRuleManifest {
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
                wasm: format!("{}/rule.wasm", mock_server.uri()),
                sha256: "ignored".to_string(),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        };

        let downloader = WasmDownloader::new();
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

        let manifest = ExternalRuleManifest {
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
                wasm: format!("{}/rule.wasm", mock_server.uri()),
                sha256: "ignored".to_string(),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        };

        let downloader = WasmDownloader::new();
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

        let manifest = ExternalRuleManifest {
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
                wasm: format!("{}/large.wasm", mock_server.uri()),
                sha256: "ignored".to_string(),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        };

        let downloader = WasmDownloader::with_max_size(max_size);
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

        // Use a chunked response or simply a body larger than limit where Content-Length might be ignored or missing
        // wiremock sets Content-Length by default for strict responses.
        // We can check if TooLarge catches it even if we just stream it.
        // But to verify *streaming* specifically, we often rely on implementation detail or large bodies.
        // Here we just verify that it fails eventually.

        Mock::given(method("GET"))
            .and(path("/stream.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_string("A".repeat(10)))
            .mount(&mock_server)
            .await;

        let manifest = ExternalRuleManifest {
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
                wasm: format!("{}/stream.wasm", mock_server.uri()),
                sha256: "ignored".to_string(),
            },
            permissions: None,
            tsuzulint: None,
            options: None,
        };

        let downloader = WasmDownloader::with_max_size(max_size);
        let result = downloader.download(&manifest).await;

        match result {
            Err(DownloadError::TooLarge { size: _, max }) => {
                assert_eq!(max, max_size);
            }
            _ => panic!("Expected TooLarge error"),
        }
    }
}
