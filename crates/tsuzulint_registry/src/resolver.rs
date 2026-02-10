//! Plugin resolver for fetching and caching plugins.

use crate::cache::{CacheError, PluginCache};
use crate::downloader::{DownloadError, WasmDownloader};
use crate::error::FetchError;
use crate::fetcher::{ManifestFetcher, PluginSource as FetcherSource};
use crate::manifest::ExternalRuleManifest;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::hash::HashError;
use std::path::{Path, PathBuf};

/// Error during plugin parsing.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid plugin specification format")]
    InvalidFormat,
    #[error("Missing alias ('as') for {src} source")]
    MissingAlias { src: String },
    #[error("Invalid object format: {0}")]
    InvalidObject(String),
}

/// Error during plugin resolution.
#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
    #[error("Fetch error: {0}")]
    FetchError(#[from] FetchError),
    #[error("Download error: {0}")]
    DownloadError(#[from] DownloadError),
    #[error("Cache error: {0}")]
    CacheError(#[from] CacheError),
    #[error("Hash mismatch: {0}")]
    HashMismatch(#[from] HashError),
    #[error("Alias required for {src}")]
    AliasRequired { src: String },
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Source of a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSource {
    /// GitHub repository source.
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

/// A parsed plugin specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    pub source: PluginSource,
    pub alias: Option<String>,
}

impl PluginSpec {
    /// Parse a plugin specification from a JSON value.
    pub fn parse(value: &Value) -> Result<Self, ParseError> {
        match value {
            Value::String(s) => Self::parse_string(s),
            Value::Object(_) => Self::parse_object(value),
            _ => Err(ParseError::InvalidFormat),
        }
    }

    fn parse_string(s: &str) -> Result<Self, ParseError> {
        // Format: "owner/repo" or "owner/repo@version"
        let parts: Vec<&str> = s.split('@').collect();
        let (name_part, version) = match parts.len() {
            1 => (parts[0], None),
            2 => {
                let v = parts[1].trim();
                if v.is_empty() {
                    return Err(ParseError::InvalidFormat);
                }
                (parts[0], Some(v.to_string()))
            }
            _ => return Err(ParseError::InvalidFormat), // Too many @
        };

        let name_parts: Vec<&str> = name_part.split('/').collect();
        if name_parts.len() != 2 {
            return Err(ParseError::InvalidFormat);
        }

        let owner = name_parts[0].trim();
        let repo = name_parts[1].trim();

        if owner.is_empty() || repo.is_empty() {
            return Err(ParseError::InvalidFormat);
        }

        Ok(Self {
            source: PluginSource::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
                version,
            },
            alias: None,
        })
    }

    fn parse_object(value: &Value) -> Result<Self, ParseError> {
        #[derive(Deserialize)]
        struct SpecObj {
            github: Option<String>,
            url: Option<String>,
            path: Option<String>,
            #[serde(rename = "as")]
            alias: Option<String>,
        }

        let obj: SpecObj = serde_json::from_value(value.clone())
            .map_err(|e| ParseError::InvalidObject(e.to_string()))?;

        // Ensure exactly one source is provided
        let sources_count = [obj.github.is_some(), obj.url.is_some(), obj.path.is_some()]
            .into_iter()
            .filter(|&x| x)
            .count();

        if sources_count != 1 {
            return Err(ParseError::InvalidObject(
                "Exactly one of 'github', 'url', or 'path' must be specified".to_string(),
            ));
        }

        if let Some(github) = obj.github {
            // Re-use string parsing logic for github field, but override version if needed?
            // The spec says "owner/rule" format for github field.
            // "github": "owner/tsuzulint-rule-no-todo"
            let spec = Self::parse_string(&github)?;
            return Ok(Self {
                source: spec.source,
                alias: obj.alias,
            });
        }

        if let Some(url) = obj.url {
            if obj.alias.is_none() {
                return Err(ParseError::MissingAlias {
                    src: "url".to_string(),
                });
            }
            return Ok(Self {
                source: PluginSource::Url(url),
                alias: obj.alias,
            });
        }

        if let Some(path_str) = obj.path {
            let mut alias = obj.alias;
            if alias.is_none() {
                // Read manifest to get name as per requirement
                let path = PathBuf::from(&path_str);
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    ParseError::InvalidObject(format!(
                        "Failed to read manifest at {}: {}",
                        path_str, e
                    ))
                })?;
                let manifest: ExternalRuleManifest =
                    serde_json::from_str(&content).map_err(|e| {
                        ParseError::InvalidObject(format!(
                            "Failed to parse manifest at {}: {}",
                            path_str, e
                        ))
                    })?;
                alias = Some(manifest.rule.name);
            }
            return Ok(Self {
                source: PluginSource::Path(PathBuf::from(path_str)),
                alias,
            });
        }

        Err(ParseError::InvalidFormat)
    }
}

/// Resolved plugin with local paths.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPlugin {
    pub wasm_path: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: ExternalRuleManifest,
    pub alias: String,
}

/// Resolver for plugins.
pub struct PluginResolver {
    fetcher: ManifestFetcher,
    cache: PluginCache,
    downloader: WasmDownloader,
}

impl PluginResolver {
    pub fn new() -> Result<Self, CacheError> {
        Ok(Self {
            fetcher: ManifestFetcher::new(),
            cache: PluginCache::new()?,
            downloader: WasmDownloader::new(),
        })
    }

    /// Create a new resolver with a custom fetcher (mainly for testing).
    pub fn with_fetcher(fetcher: ManifestFetcher) -> Result<Self, CacheError> {
        Ok(Self {
            fetcher,
            cache: PluginCache::new()?,
            downloader: WasmDownloader::new(),
        })
    }

    /// Resolve a plugin specification to a usable WASM module.
    pub async fn resolve(&self, spec: &PluginSpec) -> Result<ResolvedPlugin, ResolveError> {
        let fetcher_source = match &spec.source {
            PluginSource::GitHub {
                owner,
                repo,
                version,
            } => FetcherSource::GitHub {
                owner: owner.clone(),
                repo: repo.clone(),
                version: version.clone(),
            },
            PluginSource::Url(url) => FetcherSource::Url(url.clone()),
            PluginSource::Path(path) => FetcherSource::Path(path.clone()),
        };

        // Fail fast: strict alias check for Url sources
        if matches!(spec.source, PluginSource::Url(_)) && spec.alias.is_none() {
            return Err(ResolveError::AliasRequired {
                src: "url".to_string(),
            });
        }

        let manifest = self.fetcher.fetch(&fetcher_source).await?;

        // Determine alias (GitHub falls back to manifest name, others used validated spec alias)
        let alias = spec
            .alias
            .clone()
            .unwrap_or_else(|| manifest.rule.name.clone());

        match &spec.source {
            PluginSource::GitHub { version, .. } => {
                let version_str = version.as_deref().unwrap_or(&manifest.rule.version);

                // Try cache
                if let Some(cached) = self.cache.get(&fetcher_source, version_str) {
                    return Ok(ResolvedPlugin {
                        wasm_path: cached.wasm_path,
                        manifest_path: cached.manifest_path,
                        manifest,
                        alias,
                    });
                }

                // Download
                let result = self.downloader.download(&manifest).await?;

                // Verify hash
                if result.computed_hash != manifest.artifacts.sha256 {
                    return Err(ResolveError::HashMismatch(HashError::Mismatch {
                        expected: manifest.artifacts.sha256.clone(),
                        actual: result.computed_hash,
                    }));
                }

                // Store in cache
                let manifest_json = serde_json::to_string(&manifest)
                    .map_err(|e| ResolveError::SerializationError(e.to_string()))?;

                let cached = self.cache.store(
                    &fetcher_source,
                    version_str,
                    &result.bytes,
                    &manifest_json,
                )?;

                Ok(ResolvedPlugin {
                    wasm_path: cached.wasm_path,
                    manifest_path: cached.manifest_path,
                    manifest,
                    alias,
                })
            }
            PluginSource::Url(_) => {
                // Try cache first
                // Use manifest version as cache key since URL doesn't guarantee version in itself
                let version_str = &manifest.rule.version;

                if let Some(cached) = self.cache.get(&fetcher_source, version_str) {
                    return Ok(ResolvedPlugin {
                        wasm_path: cached.wasm_path,
                        manifest_path: cached.manifest_path,
                        manifest,
                        alias,
                    });
                }

                let result = self.downloader.download(&manifest).await?;

                if result.computed_hash != manifest.artifacts.sha256 {
                    return Err(ResolveError::HashMismatch(HashError::Mismatch {
                        expected: manifest.artifacts.sha256.clone(),
                        actual: result.computed_hash,
                    }));
                }

                // Store in cache
                let manifest_json = serde_json::to_string(&manifest)
                    .map_err(|e| ResolveError::SerializationError(e.to_string()))?;

                let cached = self.cache.store(
                    &fetcher_source,
                    version_str,
                    &result.bytes,
                    &manifest_json,
                )?;

                Ok(ResolvedPlugin {
                    wasm_path: cached.wasm_path,
                    manifest_path: cached.manifest_path,
                    manifest,
                    alias,
                })
            }
            PluginSource::Path(base_path) => {
                // Resolve relative path
                let wasm_relative_str = &manifest.artifacts.wasm;
                let wasm_relative = Path::new(wasm_relative_str);

                // 1. Reject absolute paths and '..' components
                if wasm_relative.is_absolute() {
                    return Err(ResolveError::DownloadError(DownloadError::NotFound(
                        format!(
                            "Absolute path not allowed in manifest: {}",
                            wasm_relative_str
                        ),
                    )));
                }
                if wasm_relative
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(ResolveError::DownloadError(DownloadError::NotFound(
                        format!(
                            "Parent directory '..' not allowed in manifest: {}",
                            wasm_relative_str
                        ),
                    )));
                }

                // If base_path is a file (manifest.json), we want its parent
                let parent = if base_path.is_file() {
                    base_path.parent().unwrap_or(Path::new("."))
                } else {
                    base_path.as_path()
                };

                let wasm_path = parent.join(wasm_relative);

                if !wasm_path.exists() {
                    return Err(ResolveError::DownloadError(DownloadError::NotFound(
                        wasm_path.to_string_lossy().to_string(),
                    )));
                }

                // 2. Canonicalize and verify containment
                let parent_canon = parent.canonicalize().map_err(DownloadError::IoError)?;
                let wasm_canon = wasm_path.canonicalize().map_err(DownloadError::IoError)?;

                if !wasm_canon.starts_with(&parent_canon) {
                    return Err(ResolveError::DownloadError(DownloadError::NotFound(
                        format!(
                            "Path traversal detected: {} escapes {}",
                            wasm_path.display(),
                            parent.display()
                        ),
                    )));
                }

                let bytes = std::fs::read(&wasm_path).map_err(DownloadError::IoError)?;
                let computed_hash = crate::hash::HashVerifier::compute(&bytes);

                if computed_hash != manifest.artifacts.sha256 {
                    return Err(ResolveError::HashMismatch(HashError::Mismatch {
                        expected: manifest.artifacts.sha256.clone(),
                        actual: computed_hash,
                    }));
                }

                Ok(ResolvedPlugin {
                    wasm_path,
                    manifest_path: base_path.clone(),
                    manifest,
                    alias,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_string_github_latest() {
        let value = json!("owner/repo");
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: None
            }
        );
        assert_eq!(spec.alias, None);
    }

    #[test]
    fn test_parse_string_github_version() {
        let value = json!("owner/repo@v1.2.3");
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: Some("v1.2.3".to_string())
            }
        );
    }

    #[test]
    fn test_parse_object_github() {
        let value = json!({
            "github": "owner/repo",
            "as": "my-rule"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: None
            }
        );
        assert_eq!(spec.alias, Some("my-rule".to_string()));
    }

    #[test]
    fn test_parse_object_url() {
        let value = json!({
            "url": "https://example.com/manifest.json",
            "as": "rule-alias"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::Url("https://example.com/manifest.json".to_string())
        );
        assert_eq!(spec.alias, Some("rule-alias".to_string()));
    }

    #[test]
    fn test_parse_object_path() {
        let value = json!({
            "path": "./local/rule",
            "as": "local-rule"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::Path(PathBuf::from("./local/rule"))
        );
        assert_eq!(spec.alias, Some("local-rule".to_string()));
    }

    #[test]
    fn test_parse_error_missing_alias_url() {
        let value = json!({ "url": "https://example.com" });
        let result = PluginSpec::parse(&value);
        assert!(matches!(result, Err(ParseError::MissingAlias { .. })));
    }

    #[test]
    fn test_parse_object_path_optional_alias() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let manifest = json!({
            "rule": { "name": "test-rule-name", "version": "1.0.0" },
            "artifacts": { "wasm": "rule.wasm", "sha256": "..." }
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let value = json!({
            "path": manifest_path.to_str().unwrap()
        });
        let spec = PluginSpec::parse(&value).expect("Parsing should succeed now");
        assert_eq!(spec.alias, Some("test-rule-name".to_string()));
    }

    #[test]
    fn test_parse_error_invalid_string() {
        assert!(PluginSpec::parse(&json!("invalid")).is_err());
        assert!(PluginSpec::parse(&json!("owner/repo/extra")).is_err());
        assert!(PluginSpec::parse(&json!("owner/repo@v1@v2")).is_err());

        // Empty components
        assert!(PluginSpec::parse(&json!("/repo")).is_err());
        assert!(PluginSpec::parse(&json!("owner/")).is_err());
        assert!(PluginSpec::parse(&json!("/")).is_err());
    }

    use crate::hash::HashVerifier;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_resolve_github_success() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

        // Mock Manifest
        let manifest = json!({
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "isolation_level": "global"
            },
            "artifacts": {
                "wasm": format!("{}/rule.wasm", mock_server.uri()),
                "sha256": wasm_hash
            }
        });

        // Mock Manifest Endpoint
        // ManifestFetcher uses: {base}/{owner}/{repo}/releases/latest/download/tsuzulint-rule.json
        Mock::given(method("GET"))
            .and(path(
                "/owner/repo/releases/latest/download/tsuzulint-rule.json",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
            .mount(&mock_server)
            .await;

        // Mock WASM Endpoint (referenced in manifest)
        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm_content.as_slice()))
            .mount(&mock_server)
            .await;

        // Configure fetcher to use mock server
        let fetcher = ManifestFetcher::new().with_base_url(mock_server.uri());
        let resolver = PluginResolver::with_fetcher(fetcher).unwrap();

        let spec = PluginSpec::parse(&json!("owner/repo")).unwrap();

        let resolved = resolver.resolve(&spec).await.expect("Resolve failed");

        assert_eq!(resolved.alias, "test-rule");
        assert_eq!(resolved.manifest.rule.name, "test-rule");
        assert_eq!(std::fs::read(&resolved.wasm_path).unwrap(), wasm_content);
    }

    #[tokio::test]
    async fn test_resolve_url_success() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

        // Manifest
        let manifest = json!({
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
            },
            "artifacts": {
                "wasm": format!("{}/rule.wasm", mock_server.uri()),
                "sha256": wasm_hash
            }
        });

        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm_content.as_slice()))
            .mount(&mock_server)
            .await;

        let resolver = PluginResolver::new().unwrap();
        let spec = PluginSpec::parse(&json!({
            "url": format!("{}/manifest.json", mock_server.uri()),
            "as": "test-alias"
        }))
        .unwrap();

        let resolved = resolver.resolve(&spec).await.expect("Resolve failed");

        assert_eq!(resolved.alias, "test-alias");
        assert_eq!(resolved.manifest.rule.name, "test-rule");
        assert_eq!(std::fs::read(&resolved.wasm_path).unwrap(), wasm_content);
    }

    #[tokio::test]
    async fn test_resolve_path_success() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        let wasm_content = b"local wasm";
        // Write WASM
        std::fs::write(&wasm_path, wasm_content).unwrap();

        let manifest = json!({
            "rule": {
                "name": "local-rule",
                "version": "1.0.0",
            },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": HashVerifier::compute(wasm_content)
            }
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();
        let spec = PluginSpec::parse(&json!({
            "path": manifest_path.to_str().unwrap(),
            "as": "local-alias"
        }))
        .unwrap();

        let resolved = resolver.resolve(&spec).await.expect("Resolve failed");

        assert_eq!(resolved.wasm_path, wasm_path);
        assert_eq!(resolved.alias, "local-alias");
    }

    #[tokio::test]
    async fn test_resolve_path_not_found() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        // Manifest exists but WASM does not
        let manifest = json!({
            "rule": {
                "name": "local-rule",
                "version": "1.0.0",
            },
            "artifacts": {
                "wasm": "nonexistent.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();
        let spec = PluginSpec::parse(&json!({
            "path": manifest_path.to_str().unwrap(),
            "as": "local-alias"
        }))
        .unwrap();

        let result = resolver.resolve(&spec).await;
        assert!(matches!(
            result,
            Err(ResolveError::DownloadError(DownloadError::NotFound(_)))
        ));
    }

    #[tokio::test]
    async fn test_resolve_path_hash_mismatch() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        // Write WASM
        std::fs::write(&wasm_path, b"local wasm").unwrap();

        // Manifest with WRONG hash
        let wrong_hash = "a".repeat(64); // A valid 64-char hex string that is definitely wrong.

        let manifest = json!({
            "rule": {
                "name": "local-rule",
                "version": "1.0.0",
            },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": wrong_hash
            }
        });

        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();
        let spec = PluginSpec::parse(&json!({
            "path": manifest_path.to_str().unwrap(),
            "as": "local-alias"
        }))
        .unwrap();

        let result = resolver.resolve(&spec).await;
        assert!(matches!(result, Err(ResolveError::HashMismatch(_))));
    }

    #[tokio::test]
    async fn test_resolve_path_traversal() {
        let dir = tempdir().unwrap();

        // Create a 'safe' directory where manifest lives
        let safe_dir = dir.path().join("safe");
        std::fs::create_dir(&safe_dir).unwrap();
        let manifest_path = safe_dir.join("tsuzulint-rule.json");

        // Create a file OUTSIDE safe dir
        let secret_path = dir.path().join("secret.wasm");
        std::fs::write(&secret_path, b"secret data").unwrap();

        // Manifest tries to access ../secret.wasm
        let manifest = json!({
            "rule": {
                "name": "malicious-rule",
                "version": "1.0.0",
            },
            "artifacts": {
                "wasm": "../secret.wasm",
                "sha256": HashVerifier::compute(b"secret data")
            }
        });

        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();
        let spec = PluginSpec::parse(&json!({
            "path": manifest_path.to_str().unwrap(),
            "as": "malicious"
        }))
        .unwrap();

        let result = resolver.resolve(&spec).await;

        // Should fail due to parent dir rejection or path traversal check
        // In our impl, checking ".." components happens first
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not allowed") || err.to_string().contains("traversal"));
    }

    #[tokio::test]
    async fn test_resolve_url_fail_fast_missing_alias() {
        let resolver = PluginResolver::new().unwrap();
        // Manually construct spec without alias (parse would reject this, but we test resolve safety)
        let spec = PluginSpec {
            source: PluginSource::Url("https://example.com/manifest.json".to_string()),
            alias: None,
        };

        let result = resolver.resolve(&spec).await;
        match result {
            Err(ResolveError::AliasRequired { src }) => assert_eq!(src, "url"),
            _ => panic!("Should have failed with AliasRequired"),
        }
    }

    #[tokio::test]
    async fn test_resolve_path_optional_alias() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");
        let wasm_content = b"wasm content";
        std::fs::write(&wasm_path, wasm_content).unwrap();

        let manifest = json!({
            "rule": { "name": "auto-alias", "version": "1.0.0" },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": HashVerifier::compute(wasm_content)
            }
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();
        // Manually construct spec without alias
        let spec = PluginSpec {
            source: PluginSource::Path(manifest_path),
            alias: None,
        };

        let resolved = resolver
            .resolve(&spec)
            .await
            .expect("Resolve should succeed");
        assert_eq!(resolved.alias, "auto-alias");
    }

    #[test]
    fn test_parse_string_error_empty_version() {
        assert!(matches!(
            PluginSpec::parse(&json!("owner/repo@")),
            Err(ParseError::InvalidFormat)
        ));
        assert!(matches!(
            PluginSpec::parse(&json!("owner/repo@   ")),
            Err(ParseError::InvalidFormat)
        ));
    }

    #[test]
    fn test_parse_object_error_multiple_sources() {
        let value = json!({
            "github": "owner/repo",
            "url": "https://example.com/manifest.json",
            "as": "alias"
        });
        match PluginSpec::parse(&value) {
            Err(ParseError::InvalidObject(msg)) => {
                assert!(msg.contains("Exactly one"));
            }
            _ => panic!("Should fail with InvalidObject"),
        }
    }

    #[test]
    fn test_parse_object_error_no_source() {
        let value = json!({
            "as": "alias"
        });
        match PluginSpec::parse(&value) {
            Err(ParseError::InvalidObject(msg)) => {
                assert!(msg.contains("Exactly one"));
            }
            _ => panic!("Should fail with InvalidObject"),
        }
    }
}
