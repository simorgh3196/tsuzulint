//! Manifest fetcher for plugin sources.

use crate::error::FetchError;
use crate::manifest::{ExternalRuleManifest, validate_manifest};
use std::path::PathBuf;

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
    client: reqwest::Client,
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
            client: reqwest::Client::new(),
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
        let url = match version {
            Some(v) => format!(
                "https://github.com/{owner}/{repo}/releases/download/v{v}/tsuzulint-rule.json"
            ),
            None => format!(
                "https://github.com/{owner}/{repo}/releases/latest/download/tsuzulint-rule.json"
            ),
        };

        self.fetch_from_url(&url).await
    }

    /// Fetch manifest from a URL.
    async fn fetch_from_url(&self, url: &str) -> Result<ExternalRuleManifest, FetchError> {
        let response = self.client.get(url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FetchError::NotFound(format!("Manifest not found at {url}")));
        }

        let text = response.error_for_status()?.text().await?;
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

        let content = tokio::fs::read_to_string(path).await?;
        let manifest = validate_manifest(&content)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
