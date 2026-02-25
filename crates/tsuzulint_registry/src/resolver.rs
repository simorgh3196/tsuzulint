//! Plugin resolver for fetching and caching plugins.

use crate::cache::{CacheError, PluginCache};
use crate::downloader::{DownloadError, WasmDownloader};
use crate::error::FetchError;
use crate::fetcher::ManifestFetcher;
use crate::manifest::ExternalRuleManifest;
use crate::manifest::IntegrityError;
use crate::security::validate_local_wasm_path;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use crate::fetcher::PluginSource;
pub use crate::spec::{ParseError, PluginSpec};

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
    #[error("Integrity check failed: {0}")]
    IntegrityError(#[from] IntegrityError),
    #[error("Alias required for {src}")]
    AliasRequired { src: String },
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Security error: {0}")]
    SecurityError(#[from] crate::security::SecurityError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPlugin {
    pub wasm_path: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: ExternalRuleManifest,
    pub alias: String,
}

pub struct PluginResolver {
    fetcher: ManifestFetcher,
    cache: PluginCache,
    downloader: WasmDownloader,
}

impl PluginResolver {
    pub fn new() -> Result<Self, ResolveError> {
        Ok(Self {
            fetcher: ManifestFetcher::new(),
            cache: PluginCache::new()?,
            downloader: WasmDownloader::new()?,
        })
    }

    pub fn with_fetcher(fetcher: ManifestFetcher) -> Result<Self, ResolveError> {
        Ok(Self {
            fetcher,
            cache: PluginCache::new()?,
            downloader: WasmDownloader::new()?,
        })
    }

    pub fn with_downloader(mut self, downloader: WasmDownloader) -> Self {
        self.downloader = downloader;
        self
    }

    pub async fn resolve(&self, spec: &PluginSpec) -> Result<ResolvedPlugin, ResolveError> {
        let (fetcher_source, manifest_path_buf) = self.prepare_source(&spec.source);

        if matches!(spec.source, PluginSource::Url(_)) && spec.alias.is_none() {
            return Err(ResolveError::AliasRequired {
                src: "url".to_string(),
            });
        }

        let manifest = self.fetcher.fetch(&fetcher_source).await?;

        let alias = spec
            .alias
            .clone()
            .unwrap_or_else(|| manifest.rule.name.clone());

        match &spec.source {
            PluginSource::GitHub { version, .. } => {
                let version_str = version.as_ref().unwrap_or(&manifest.rule.version).clone();
                self.resolve_remote(&fetcher_source, &version_str, manifest, alias)
                    .await
            }
            PluginSource::Url(_) => {
                let version_str = manifest.rule.version.clone();
                self.resolve_remote(&fetcher_source, &version_str, manifest, alias)
                    .await
            }
            PluginSource::Path(_) => {
                let manifest_path = manifest_path_buf.expect("Path source must have manifest path");
                self.resolve_local(&manifest_path, &manifest, alias)
            }
        }
    }

    fn prepare_source(&self, source: &PluginSource) -> (PluginSource, Option<PathBuf>) {
        match source {
            PluginSource::GitHub {
                owner,
                repo,
                version,
            } => (
                PluginSource::GitHub {
                    owner: owner.clone(),
                    repo: repo.clone(),
                    version: version.clone(),
                },
                None,
            ),
            PluginSource::Url(url) => (PluginSource::Url(url.clone()), None),
            PluginSource::Path(path) => {
                let p = if path.is_dir() {
                    path.join("tsuzulint-rule.json")
                } else {
                    path.clone()
                };
                (PluginSource::Path(p.clone()), Some(p))
            }
        }
    }

    async fn resolve_remote(
        &self,
        source: &PluginSource,
        version: &str,
        manifest: ExternalRuleManifest,
        alias: String,
    ) -> Result<ResolvedPlugin, ResolveError> {
        if let Some(cached) = self.cache.get(source, version) {
            return Ok(ResolvedPlugin {
                wasm_path: cached.wasm_path,
                manifest_path: cached.manifest_path,
                manifest,
                alias,
            });
        }

        let result = self.downloader.download(&manifest).await?;

        if result.computed_hash != manifest.artifacts.sha256 {
            return Err(ResolveError::IntegrityError(IntegrityError::HashMismatch {
                expected: manifest.artifacts.sha256.clone(),
                actual: result.computed_hash,
            }));
        }

        let manifest_json = serde_json::to_string(&manifest)
            .map_err(|e| ResolveError::SerializationError(e.to_string()))?;

        let cached = self
            .cache
            .store(source, version, &result.bytes, &manifest_json)?;

        Ok(ResolvedPlugin {
            wasm_path: cached.wasm_path,
            manifest_path: cached.manifest_path,
            manifest,
            alias,
        })
    }

    fn resolve_local(
        &self,
        manifest_path: &Path,
        manifest: &ExternalRuleManifest,
        alias: String,
    ) -> Result<ResolvedPlugin, ResolveError> {
        let wasm_relative = Path::new(&manifest.artifacts.wasm);
        let parent = manifest_path.parent().unwrap_or(Path::new("."));

        let wasm_path = validate_local_wasm_path(wasm_relative, parent)?;

        let bytes = std::fs::read(&wasm_path).map_err(DownloadError::IoError)?;
        let computed_hash = tsuzulint_manifest::HashVerifier::compute(&bytes);

        if computed_hash != manifest.artifacts.sha256 {
            return Err(ResolveError::IntegrityError(IntegrityError::HashMismatch {
                expected: manifest.artifacts.sha256.clone(),
                actual: computed_hash,
            }));
        }

        Ok(ResolvedPlugin {
            wasm_path,
            manifest_path: manifest_path.to_path_buf(),
            manifest: manifest.clone(),
            alias,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityError;
    use serde_json::json;
    use tempfile::tempdir;
    use tsuzulint_manifest::HashVerifier;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_resolve_github_success() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

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

        Mock::given(method("GET"))
            .and(path(
                "/owner/repo/releases/latest/download/tsuzulint-rule.json",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/rule.wasm"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(wasm_content.as_slice()))
            .mount(&mock_server)
            .await;

        let fetcher = ManifestFetcher::new()
            .with_base_url(mock_server.uri())
            .allow_local(true);

        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader);

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

        let fetcher = ManifestFetcher::new().allow_local(true);
        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader);

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

        assert_eq!(
            resolved.wasm_path.canonicalize().unwrap(),
            wasm_path.canonicalize().unwrap()
        );
        assert_eq!(resolved.alias, "local-alias");
    }

    #[tokio::test]
    async fn test_resolve_path_not_found() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

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
            Err(ResolveError::SecurityError(
                SecurityError::FileNotFound { .. }
            ))
        ));
    }

    #[tokio::test]
    async fn test_resolve_path_hash_mismatch() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        std::fs::write(&wasm_path, b"local wasm").unwrap();

        let wrong_hash = "a".repeat(64);

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
        assert!(matches!(result, Err(ResolveError::IntegrityError(_))));
    }

    #[tokio::test]
    async fn test_resolve_path_traversal() {
        let dir = tempdir().unwrap();

        let safe_dir = dir.path().join("safe");
        std::fs::create_dir(&safe_dir).unwrap();
        let manifest_path = safe_dir.join("tsuzulint-rule.json");

        let secret_path = dir.path().join("secret.wasm");
        std::fs::write(&secret_path, b"secret data").unwrap();

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

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not allowed") || err.to_string().contains("traversal"));
    }

    #[tokio::test]
    async fn test_resolve_url_fail_fast_missing_alias() {
        let resolver = PluginResolver::new().unwrap();
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

    #[tokio::test]
    async fn test_resolve_path_directory() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");
        let wasm_content = b"wasm content";
        std::fs::write(&wasm_path, wasm_content).unwrap();

        let manifest = json!({
            "rule": { "name": "dir-rule", "version": "1.0.0" },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": HashVerifier::compute(wasm_content)
            }
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();

        let spec = PluginSpec {
            source: PluginSource::Path(dir.path().to_path_buf()),
            alias: None,
        };

        let resolved = resolver
            .resolve(&spec)
            .await
            .expect("Resolve should succeed with directory path");
        assert_eq!(resolved.alias, "dir-rule");
        assert!(resolved.manifest_path.ends_with("tsuzulint-rule.json"));
    }
}
