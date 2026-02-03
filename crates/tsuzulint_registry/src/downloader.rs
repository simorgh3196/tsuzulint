//! WASM downloader for plugin artifacts.

use crate::manifest::ExternalRuleManifest;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Default maximum file size for WASM downloads (10 MB).
pub const DEFAULT_MAX_SIZE: u64 = 10 * 1024 * 1024;

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
        }
    }

    /// Create a new WASM downloader with a custom maximum file size.
    pub fn with_max_size(max_size: u64) -> Self {
        Self {
            client: reqwest::Client::new(),
            max_size,
        }
    }

    /// Download WASM from the manifest's artifact URL.
    ///
    /// This method:
    /// 1. Replaces `{version}` placeholder in the URL with the manifest version
    /// 2. Downloads the WASM binary
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

    /// Download WASM from a resolved URL.
    async fn download_from_url(&self, url: &str) -> Result<DownloadResult, DownloadError> {
        let response = self.client.get(url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(DownloadError::NotFound(format!(
                "WASM file not found at {url}"
            )));
        }

        // Check Content-Length header if available
        if let Some(content_length) = response.content_length()
            && content_length > self.max_size
        {
            return Err(DownloadError::TooLarge {
                size: content_length,
                max: self.max_size,
            });
        }

        let bytes = response.error_for_status()?.bytes().await?.to_vec();

        // Check actual size after download
        if bytes.len() as u64 > self.max_size {
            return Err(DownloadError::TooLarge {
                size: bytes.len() as u64,
                max: self.max_size,
            });
        }

        // Compute SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = hasher.finalize();
        let computed_hash = hex::encode(hash);

        Ok(DownloadResult {
            bytes,
            computed_hash,
        })
    }
}

/// Convert a byte slice to lowercase hex string.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_hex_encode() {
        let bytes = [0x00, 0x01, 0x02, 0xab, 0xcd, 0xef];
        assert_eq!(hex::encode(bytes), "000102abcdef");
    }

    #[test]
    fn test_sha256_computation() {
        // SHA256 of empty bytes
        let mut hasher = Sha256::new();
        hasher.update(b"");
        let hash = hasher.finalize();
        let computed = hex::encode(hash);

        // Known SHA256 of empty string
        assert_eq!(
            computed,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
