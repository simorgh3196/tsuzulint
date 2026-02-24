//! Manifest fetcher for plugin sources.

use crate::error::FetchError;
use crate::manifest::{ExternalRuleManifest, validate_manifest};
use crate::security::{check_ip, validate_url};
use reqwest::Url;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::lookup_host;

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
    github_base_url: String,
    allow_local: bool,
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
            github_base_url: "https://github.com".to_string(),
            allow_local: false,
        }
    }

    /// Set the base URL for GitHub requests (for testing).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.github_base_url = url.into();
        self
    }

    /// Configure whether to allow fetching from local network addresses.
    pub fn allow_local(mut self, allow: bool) -> Self {
        self.allow_local = allow;
        self
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
        let mut current_url_str = url_str.to_string();
        let mut redirect_count = 0;
        const MAX_REDIRECTS: u32 = 10;
        const TIMEOUT: Duration = Duration::from_secs(10);

        loop {
            if redirect_count >= MAX_REDIRECTS {
                return Err(FetchError::RedirectLimitExceeded);
            }

            let url = Url::parse(&current_url_str)
                .map_err(|e| FetchError::NotFound(format!("Invalid URL: {}", e)))?;

            // 1. Basic validation
            validate_url(&url, self.allow_local)?;

            let client = if !self.allow_local {
                if let Some(host) = url.host_str() {
                    let port = url.port_or_known_default().unwrap_or(80);

                    // 2. DNS Resolution
                    let addrs: Vec<_> = tokio::time::timeout(TIMEOUT, lookup_host((host, port)))
                        .await
                        .map_err(|_| {
                            std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "DNS lookup timed out",
                            )
                        })??
                        .collect();

                    if addrs.is_empty() {
                        return Err(FetchError::DnsNoAddress(host.to_string()));
                    }

                    // 3. IP Validation (SSRF protection)
                    for addr in &addrs {
                        check_ip(addr.ip())?;
                    }

                    // 4. Pinning for DNS Rebinding protection
                    reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .resolve_to_addrs(host, &addrs)
                        .build()
                        .map_err(FetchError::NetworkError)?
                } else {
                    // Should be unreachable for http/https due to validate_url
                    self.client.clone()
                }
            } else {
                // Allow local: use default client but with no redirects to support manual handling
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(FetchError::NetworkError)?
            };

            // 5. Perform Request
            let response = client.get(url.clone()).timeout(TIMEOUT).send().await?;

            // 6. Handle Redirects
            if response.status().is_redirection() {
                if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                    let location_str = location.to_str().map_err(|_| {
                        FetchError::InvalidRedirectUrl(
                            "Location header is not valid UTF-8".to_string(),
                        )
                    })?;

                    let next_url = url.join(location_str).map_err(|e| {
                        FetchError::InvalidRedirectUrl(format!(
                            "Failed to parse redirect URL: {}",
                            e
                        ))
                    })?;

                    current_url_str = next_url.to_string();
                    redirect_count += 1;
                    continue;
                }
            }

            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(FetchError::NotFound(format!(
                    "Manifest not found at {current_url_str}"
                )));
            }

            let text = response.error_for_status()?.text().await?;
            let manifest = validate_manifest(&text)?;
            return Ok(manifest);
        }
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
        use wiremock::MockServer;

        let mock_server = MockServer::start().await;

        let fetcher = ManifestFetcher::new();
        let url = format!("{}/manifest.json", mock_server.uri());
        let result = fetcher.fetch(&PluginSource::Url(url)).await;

        match result {
            Err(FetchError::SecurityError(SecurityError::LoopbackDenied(_))) => {}
            res => panic!("Expected SecurityError::LoopbackDenied, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_fetch_redirect_loop() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/loop"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", format!("{}/loop", mock_server.uri())),
            )
            .mount(&mock_server)
            .await;

        let fetcher = ManifestFetcher::new().allow_local(true);
        let url = format!("{}/loop", mock_server.uri());
        let result = fetcher.fetch(&PluginSource::Url(url)).await;

        match result {
            Err(FetchError::RedirectLimitExceeded) => {}
            res => panic!("Expected RedirectLimitExceeded, got {:?}", res),
        }
    }

    #[tokio::test]
    async fn test_fetch_redirect_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Redirect /redirect -> /manifest.json
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", format!("{}/manifest.json", mock_server.uri())),
            )
            .mount(&mock_server)
            .await;

        // Serve valid manifest at /manifest.json
        let manifest_json = r#"{
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "description": "Test Rule",
                "fixable": false,
                "node_types": [],
                "isolation_level": "global"
            },
            "artifacts": {
                "wasm": "http://example.com/rule.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;

        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(manifest_json))
            .mount(&mock_server)
            .await;

        let fetcher = ManifestFetcher::new().allow_local(true);
        let url = format!("{}/redirect", mock_server.uri());
        let result = fetcher.fetch(&PluginSource::Url(url)).await;

        match result {
            Ok(manifest) => assert_eq!(manifest.rule.name, "test-rule"),
            Err(e) => panic!("Fetch failed: {:?}", e),
        }
    }
}
