//! Manifest fetcher for plugin sources.

use crate::error::FetchError;
use crate::http_client::{DEFAULT_MAX_REDIRECTS, DEFAULT_TIMEOUT, SecureHttpClient};
use crate::manifest::{ExternalRuleManifest, validate_manifest};
use std::path::PathBuf;
use std::time::Duration;

/// Maximum size for a manifest file (10MB).
const MAX_MANIFEST_SIZE: u64 = 10 * 1024 * 1024;

/// Source of a plugin manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSource {
    /// GitHub repository source.
    /// Format: `owner/repo` or `owner/repo@version`
    GitHub {
        owner: String,
        repo: String,
        version: Option<String>,
    },
    /// Direct URL to a manifest file.
    Url(String),
    /// Local filesystem path to a manifest file.
    Path(PathBuf),
}

impl PluginSource {
    /// Create a new GitHub source.
    pub fn github(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self::GitHub {
            owner: owner.into(),
            repo: repo.into(),
            version: None,
        }
    }

    /// Create a new GitHub source with a specific version.
    pub fn github_with_version(
        owner: impl Into<String>,
        repo: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self::GitHub {
            owner: owner.into(),
            repo: repo.into(),
            version: Some(version.into()),
        }
    }

    /// Create a new URL source.
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// Create a new path source.
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }
}

/// Fetcher for plugin manifests from various sources.
pub struct ManifestFetcher {
    http_client: SecureHttpClient,
    github_base_url: String,
    timeout: Duration,
    max_redirects: u32,
}

impl Default for ManifestFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ManifestFetcher {
    /// Create a new manifest fetcher.
    pub fn new() -> Self {
        Self {
            http_client: SecureHttpClient::builder()
                .timeout(DEFAULT_TIMEOUT)
                .max_redirects(DEFAULT_MAX_REDIRECTS)
                .build(),
            github_base_url: "https://github.com".to_string(),
            timeout: DEFAULT_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }

    /// Set the base URL for GitHub requests (for testing).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.github_base_url = url.into();
        self
    }

    /// Configure whether to allow fetching from local network addresses.
    pub fn allow_local(self, allow: bool) -> Self {
        Self {
            http_client: SecureHttpClient::builder()
                .timeout(self.timeout)
                .max_redirects(self.max_redirects)
                .allow_local(allow)
                .build(),
            github_base_url: self.github_base_url,
            timeout: self.timeout,
            max_redirects: self.max_redirects,
        }
    }

    /// Fetch a manifest from the given source.
    pub async fn fetch(&self, source: &PluginSource) -> Result<ExternalRuleManifest, FetchError> {
        match source {
            PluginSource::GitHub {
                owner,
                repo,
                version,
            } => {
                self.fetch_from_github(owner, repo, version.as_deref())
                    .await
            }
            PluginSource::Url(url) => self.fetch_from_url(url).await,
            PluginSource::Path(path) => self.fetch_from_path(path).await,
        }
    }

    /// Fetch manifest from GitHub Releases.
    async fn fetch_from_github(
        &self,
        owner: &str,
        repo: &str,
        version: Option<&str>,
    ) -> Result<ExternalRuleManifest, FetchError> {
        let base = &self.github_base_url;
        let url = match version {
            Some(v) => format!("{base}/{owner}/{repo}/releases/download/v{v}/tsuzulint-rule.json"),
            None => format!("{base}/{owner}/{repo}/releases/latest/download/tsuzulint-rule.json"),
        };

        self.fetch_from_url(&url).await
    }

    /// Fetch manifest from a URL.
    async fn fetch_from_url(&self, url_str: &str) -> Result<ExternalRuleManifest, FetchError> {
        let bytes = self
            .http_client
            .fetch_with_size_limit(url_str, MAX_MANIFEST_SIZE)
            .await?;
        let text = String::from_utf8(bytes)?;
        let manifest = validate_manifest(&text)?;
        Ok(manifest)
    }

    /// Fetch manifest from a local file path.
    async fn fetch_from_path(&self, path: &PathBuf) -> Result<ExternalRuleManifest, FetchError> {
        if !path.exists() {
            return Err(FetchError::NotFound(format!(
                "Manifest file not found: {}",
                path.display()
            )));
        }

        use tokio::io::AsyncReadExt;
        let mut file = tokio::fs::File::open(path).await?;
        let mut content = String::new();
        let read = file
            .take(MAX_MANIFEST_SIZE + 1)
            .read_to_string(&mut content)
            .await?;
        if read as u64 > MAX_MANIFEST_SIZE {
            return Err(FetchError::IoError(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!(
                    "Manifest file too large: {} bytes (max {} bytes)",
                    read, MAX_MANIFEST_SIZE
                ),
            )));
        }

        let manifest = validate_manifest(&content)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityError;

    #[test]
    fn test_plugin_source_github() {
        let source = PluginSource::github("simorgh3196", "tsuzulint-rule-no-todo");
        assert_eq!(
            source,
            PluginSource::GitHub {
                owner: "simorgh3196".to_string(),
                repo: "tsuzulint-rule-no-todo".to_string(),
                version: None,
            }
        );
    }

    #[test]
    fn test_plugin_source_github_with_version() {
        let source =
            PluginSource::github_with_version("simorgh3196", "tsuzulint-rule-no-todo", "1.0.0");
        assert_eq!(
            source,
            PluginSource::GitHub {
                owner: "simorgh3196".to_string(),
                repo: "tsuzulint-rule-no-todo".to_string(),
                version: Some("1.0.0".to_string()),
            }
        );
    }

    #[test]
    fn test_plugin_source_url() {
        let source = PluginSource::url("https://example.com/tsuzulint-rule.json");
        assert_eq!(
            source,
            PluginSource::Url("https://example.com/tsuzulint-rule.json".to_string())
        );
    }

    #[test]
    fn test_plugin_source_path() {
        let source = PluginSource::path("./local/tsuzulint-rule.json");
        assert_eq!(
            source,
            PluginSource::Path(PathBuf::from("./local/tsuzulint-rule.json"))
        );
    }

    #[tokio::test]
    async fn test_fetch_from_path_success() {
        // Use our test fixture with a valid URI-format wasm field
        let manifest_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid-manifest.json");

        eprintln!("Testing with manifest path: {}", manifest_path.display());

        let fetcher = ManifestFetcher::new();
        let result = fetcher.fetch(&PluginSource::Path(manifest_path)).await;

        match &result {
            Ok(manifest) => {
                assert_eq!(manifest.rule.name, "test-rule");
            }
            Err(e) => {
                panic!("Failed to fetch manifest: {e}");
            }
        }
    }

    #[tokio::test]
    async fn test_fetch_from_path_not_found() {
        let fetcher = ManifestFetcher::new();
        let result = fetcher
            .fetch(&PluginSource::Path(PathBuf::from(
                "/nonexistent/tsuzulint-rule.json",
            )))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            FetchError::NotFound(msg) => {
                assert!(msg.contains("not found"));
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_fetch_from_path_invalid_manifest() {
        // Use our test fixture with an invalid manifest (missing required fields)
        let manifest_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/invalid-manifest.json");

        let fetcher = ManifestFetcher::new();
        let result = fetcher.fetch(&PluginSource::Path(manifest_path)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            FetchError::InvalidManifest(_) => {}
            e => panic!("Expected InvalidManifest error, got: {e}"),
        }
    }

    #[tokio::test]
    async fn test_fetch_local_denied_by_default() {
        use crate::http_client::SecureFetchError;
        use wiremock::MockServer;

        let mock_server = MockServer::start().await;

        let fetcher = ManifestFetcher::new();
        let url = format!("{}/manifest.json", mock_server.uri());
        let result = fetcher.fetch(&PluginSource::Url(url)).await;

        match result {
            Err(FetchError::SecureFetchError(SecureFetchError::SecurityError(
                SecurityError::LoopbackDenied(_),
            ))) => {}
            res => panic!(
                "Expected SecureFetchError::SecurityError::LoopbackDenied, got {:?}",
                res
            ),
        }
    }

    #[tokio::test]
    async fn test_fetch_from_url_too_large() {
        use crate::http_client::SecureFetchError;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        // Mock a response slightly larger than MAX_MANIFEST_SIZE to trigger the limit.
        let large_manifest = " ".repeat((MAX_MANIFEST_SIZE + 100) as usize);

        Mock::given(method("GET"))
            .and(path("/large.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&large_manifest))
            .mount(&mock_server)
            .await;

        let fetcher = ManifestFetcher::new().allow_local(true);
        let url = format!("{}/large.json", mock_server.uri());
        let result = fetcher.fetch(&PluginSource::Url(url)).await;

        match result {
            Err(FetchError::SecureFetchError(SecureFetchError::ResponseTooLarge { size, max })) => {
                assert!(size > max);
                assert_eq!(max, MAX_MANIFEST_SIZE);
            }
            res => panic!("Expected ResponseTooLarge error, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_fetch_from_path_too_large() {
        use tempfile::NamedTempFile;

        // Create a file slightly larger than MAX_MANIFEST_SIZE
        let file = NamedTempFile::new().expect("failed to create temp file");
        let target_size = MAX_MANIFEST_SIZE + 1;
        file.as_file()
            .set_len(target_size)
            .expect("failed to resize temp file");

        let fetcher = ManifestFetcher::new();
        let result = fetcher
            .fetch(&PluginSource::Path(file.path().to_path_buf()))
            .await;

        match result {
            Err(FetchError::IoError(e)) => {
                assert_eq!(e.kind(), std::io::ErrorKind::FileTooLarge);
            }
            res => panic!("Expected IoError(FileTooLarge), got {:?}", res),
        }
    }
}
