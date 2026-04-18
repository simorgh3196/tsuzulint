//! Plugin resolver for fetching and caching plugins.

use crate::cache::{CacheError, PluginCache};
use crate::downloader::{DownloadError, WasmDownloader};
use crate::error::FetchError;
use crate::fetcher::ManifestFetcher;
use crate::manifest::{ExternalRuleManifest, HashVerifier, IntegrityError};
use crate::security::validate_local_wasm_path;
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Reads a WASM file from disk with hardening against several classes of
/// resource-exhaustion and TOCTOU attacks.
///
/// The helper applies, in order:
///
/// 1. **Non-blocking open on Unix (`O_NONBLOCK`)** so that a FIFO / named pipe
///    at the target path does not block the process indefinitely.  The flag
///    is cleared immediately after metadata validation so that the subsequent
///    read blocks normally.
/// 2. **Regular-file check** (`metadata.is_file()`) to reject FIFOs, block
///    devices, directories, etc., even if they passed the `O_NONBLOCK` open.
/// 3. **Pre-read size check** (`metadata.len() <= max_size`) which is a fast
///    fail path for obviously oversized files.
/// 4. **Bounded read** via `Read::take(max_size + 1)` so that even if a
///    symlink target is swapped between the metadata check and the read
///    (TOCTOU), or a pseudo-file like `/dev/zero` yields arbitrarily many
///    bytes with a reported size of 0, no more than `max_size + 1` bytes are
///    ever buffered.
/// 5. **Post-read size check** as a belt-and-suspenders guard against the
///    same pseudo-file and symlink-swap scenarios.
fn read_wasm_file_bounded(path: &Path, max_size: u64) -> Result<Vec<u8>, DownloadError> {
    let file = open_wasm_nonblocking(path)?;
    let metadata = file.metadata()?;

    if !metadata.is_file() {
        return Err(DownloadError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("not a regular file: {}", path.display()),
        )));
    }

    if metadata.len() > max_size {
        return Err(DownloadError::TooLarge {
            size: metadata.len(),
            max: max_size,
        });
    }

    #[cfg(unix)]
    clear_nonblocking(&file)?;

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::take(file, max_size + 1).read_to_end(&mut bytes)?;

    if bytes.len() as u64 > max_size {
        return Err(DownloadError::TooLarge {
            size: bytes.len() as u64,
            max: max_size,
        });
    }

    Ok(bytes)
}

/// Opens a file with `O_NONBLOCK` on Unix so that opening a FIFO (or other
/// special file whose producer is absent) does not block.  On non-Unix
/// platforms this is a plain `File::open`.
fn open_wasm_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(path)
    }
}

/// Clears the `O_NONBLOCK` flag on an open file descriptor so that subsequent
/// reads block normally.  Unix only; non-Unix callers need not invoke it.
#[cfg(unix)]
fn clear_nonblocking(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd as _;
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is a borrowed, owned fd for the lifetime of `file`; we only
    // clear the O_NONBLOCK bit, which does not invalidate the fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let new_flags = flags & !libc::O_NONBLOCK;
    // SAFETY: same rationale as above.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, new_flags) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub use crate::fetcher::PluginSource;
pub use crate::spec::{ParseError, PluginSpec};
use extism_manifest::Wasm;

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

    pub fn with_cache(mut self, cache: PluginCache) -> Self {
        self.cache = cache;
        self
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
                let manifest_path = manifest_path_buf.ok_or_else(|| {
                    ResolveError::SerializationError("Path source must have manifest path".into())
                })?;
                self.resolve_local(&manifest_path, &manifest, alias)
            }
        }
    }

    fn prepare_source(&self, source: &PluginSource) -> (PluginSource, Option<PathBuf>) {
        match source {
            PluginSource::GitHub { .. } | PluginSource::Url(_) => (source.clone(), None),
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
        let (expected_hash, mut wasm_url) = manifest
            .wasm
            .iter()
            .find_map(|w| match w {
                Wasm::Url { req, meta } => meta
                    .hash
                    .as_ref()
                    .map(|hash| (hash.clone(), req.url.clone())),
                Wasm::File { .. } | Wasm::Data { .. } => None,
            })
            .ok_or_else(|| {
                ResolveError::SerializationError(
                    "Missing hash or URL in external manifest for URL source".into(),
                )
            })?;

        wasm_url = wasm_url.replace("{version}", &manifest.rule.version);

        if let Some(cached) = self.cache.get(source, version) {
            // The cache directory is written exclusively by us (see `PluginCache::store`),
            // but we still treat on-disk contents as untrusted: another process, a
            // symlink attack, or accidental corruption could have replaced the cached
            // artefact with an oversized / special file.  Applying the same bounded
            // read as the local-path branch (L236) prevents a DoS via an attacker
            // who can plant or swap a file in the cache directory.
            let max_size = crate::downloader::DEFAULT_MAX_SIZE;
            match read_wasm_file_bounded(&cached.wasm_path, max_size) {
                Ok(cached_bytes) => {
                    if HashVerifier::verify(&cached_bytes, &expected_hash).is_ok() {
                        return Ok(ResolvedPlugin {
                            wasm_path: cached.wasm_path,
                            manifest_path: cached.manifest_path,
                            manifest,
                            alias,
                        });
                    } else {
                        tracing::warn!(
                            "Cache integrity check failed for {:?}, re-downloading",
                            cached.wasm_path
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to read cached WASM at {:?}: {}, re-downloading",
                        cached.wasm_path,
                        e
                    );
                }
            }
        }

        let result = self.downloader.download(&wasm_url).await?;

        HashVerifier::verify(&result.bytes, &expected_hash)?;

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
        let (wasm_file, expected_hash) = manifest
            .wasm
            .iter()
            .find_map(|w| match w {
                Wasm::File { path, meta } => Some((path.clone(), meta.hash.clone())),
                Wasm::Url { .. } | Wasm::Data { .. } => None,
            })
            .ok_or_else(|| {
                ResolveError::SerializationError(
                    "Local resolution requires a file path source (Wasm::File), but none was found in manifest".into()
                )
            })?;

        let wasm_relative = Path::new(&wasm_file);
        let parent = manifest_path.parent().unwrap_or(Path::new("."));

        let wasm_path = validate_local_wasm_path(wasm_relative, parent)?;

        let max_size = crate::downloader::DEFAULT_MAX_SIZE;
        let bytes = read_wasm_file_bounded(&wasm_path, max_size)?;

        let expected_hash = expected_hash.ok_or_else(|| {
            ResolveError::SerializationError(
                "Missing hash in external manifest for local file".into(),
            )
        })?;

        HashVerifier::verify(&bytes, &expected_hash)?;

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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn github_spec_with_server_url(server_url: &str) -> PluginSpec {
        PluginSpec::parse(&json!({
            "github": "owner/repo",
            "server_url": server_url
        }))
        .unwrap()
    }

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
            "wasm": [{
                "url": format!("{}/rule.wasm", mock_server.uri()),
                "hash": wasm_hash
            }]
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

        let fetcher = ManifestFetcher::new().allow_local(true);

        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let dir = tempdir().unwrap();
        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader)
            .with_cache(PluginCache::with_dir(dir.path()));

        let spec = github_spec_with_server_url(&mock_server.uri());

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
            "wasm": [{
                "url": format!("{}/rule.wasm", mock_server.uri()),
                "hash": wasm_hash
            }]
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

        let dir = tempdir().unwrap();
        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader)
            .with_cache(PluginCache::with_dir(dir.path()));

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
            "wasm": [{
                "path": "rule.wasm",
                "hash": HashVerifier::compute(wasm_content)
            }]
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));
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
            "wasm": [{
                "path": "nonexistent.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));
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
            "wasm": [{
                "path": "rule.wasm",
                "hash": wrong_hash
            }]
        });

        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));
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
            "wasm": [{
                "path": "../secret.wasm",
                "hash": HashVerifier::compute(b"secret data")
            }]
        });

        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));
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
        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));
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
            "wasm": [{
                "path": "rule.wasm",
                "hash": HashVerifier::compute(wasm_content)
            }]
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));
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
            "wasm": [{
                "path": "rule.wasm",
                "hash": HashVerifier::compute(wasm_content)
            }]
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let cache_dir = tempdir().unwrap();
        let resolver = PluginResolver::new()
            .unwrap()
            .with_cache(PluginCache::with_dir(cache_dir.path()));

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

    #[tokio::test]
    async fn test_resolve_cached_valid_wasm() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

        let manifest = json!({
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "isolation_level": "global"
            },
            "wasm": [{
                "url": format!("{}/rule.wasm", mock_server.uri()),
                "hash": wasm_hash
            }]
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

        let fetcher = ManifestFetcher::new().allow_local(true);

        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let dir = tempdir().unwrap();
        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader)
            .with_cache(PluginCache::with_dir(dir.path()));

        let spec = github_spec_with_server_url(&mock_server.uri());

        let resolved1 = resolver.resolve(&spec).await.expect("First resolve failed");
        let resolved2 = resolver
            .resolve(&spec)
            .await
            .expect("Second resolve failed");

        assert_eq!(resolved1.wasm_path, resolved2.wasm_path);
        assert_eq!(resolved1.manifest_path, resolved2.manifest_path);
    }

    #[tokio::test]
    async fn test_resolve_cache_tampered_redownloads() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

        let manifest = json!({
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "isolation_level": "global"
            },
            "wasm": [{
                "url": format!("{}/rule.wasm", mock_server.uri()),
                "hash": wasm_hash
            }]
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

        let fetcher = ManifestFetcher::new().allow_local(true);

        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path().to_path_buf());

        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader)
            .with_cache(cache);

        let spec = github_spec_with_server_url(&mock_server.uri());

        let resolved1 = resolver.resolve(&spec).await.expect("First resolve failed");

        std::fs::write(&resolved1.wasm_path, b"tampered content").unwrap();

        let resolved2 = resolver
            .resolve(&spec)
            .await
            .expect("Second resolve failed");

        let wasm_bytes = std::fs::read(&resolved2.wasm_path).unwrap();
        assert_eq!(wasm_bytes, wasm_content);
    }

    #[tokio::test]
    async fn test_resolve_cache_deleted_redownloads() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

        let manifest = json!({
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "isolation_level": "global"
            },
            "wasm": [{
                "url": format!("{}/rule.wasm", mock_server.uri()),
                "hash": wasm_hash
            }]
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

        let fetcher = ManifestFetcher::new().allow_local(true);

        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path().to_path_buf());

        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader)
            .with_cache(cache);

        let spec = github_spec_with_server_url(&mock_server.uri());

        let resolved1 = resolver.resolve(&spec).await.expect("First resolve failed");

        std::fs::remove_file(&resolved1.wasm_path).unwrap();
        assert!(!resolved1.wasm_path.exists(), "File should be removed");

        let resolved2 = resolver
            .resolve(&spec)
            .await
            .expect("Second resolve failed");

        assert!(resolved2.wasm_path.exists());
        let wasm_bytes = std::fs::read(&resolved2.wasm_path).unwrap();
        assert_eq!(wasm_bytes, wasm_content);
    }

    #[tokio::test]
    async fn test_resolve_path_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        // Create a file larger than max size
        let max_size = crate::downloader::DEFAULT_MAX_SIZE;
        let file = std::fs::File::create(&wasm_path).unwrap();
        file.set_len(max_size + 1).unwrap();

        let manifest = serde_json::json!({
            "rule": {
                "name": "local-rule",
                "version": "1.0.0",
            },
            "wasm": [{
                "path": "rule.wasm",
                "hash": HashVerifier::compute(b"") // hash doesn't matter, we should fail before verifying
            }]
        });
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        let resolver = PluginResolver::new().unwrap();
        let spec = PluginSpec::parse(&serde_json::json!({
            "path": manifest_path.to_str().unwrap(),
            "as": "local-rule"
        }))
        .unwrap();

        let result = resolver.resolve(&spec).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ResolveError::DownloadError(DownloadError::TooLarge { size, max }) => {
                assert_eq!(size, max_size + 1);
                assert_eq!(max, max_size);
            }
            e => panic!("Expected DownloadError::TooLarge, got {:?}", e),
        }
    }

    // -----------------------------------------------------------------------
    // Direct tests for `read_wasm_file_bounded` — exercised from both the
    // cache-hit path (L167) and the local-resolve path (L236).  Using the
    // helper directly keeps the tests fast (no manifest / HTTP mock setup)
    // and lets us deterministically trigger edge cases that would be racy
    // to reproduce through the full resolver.
    // -----------------------------------------------------------------------

    /// File whose size is exactly `DEFAULT_MAX_SIZE` must be accepted — this
    /// is the upper boundary of the allowed range.  A small `max_size` is
    /// used so the test remains fast while still exercising the exact
    /// `metadata.len() == max_size` branch.
    #[test]
    fn test_read_wasm_bounded_boundary_accept() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("boundary.wasm");
        let content = vec![0u8; 1024];
        std::fs::write(&path, &content).unwrap();

        let bytes =
            read_wasm_file_bounded(&path, 1024).expect("1024-byte file at limit must be accepted");
        assert_eq!(bytes.len(), 1024);
    }

    /// File whose size is `DEFAULT_MAX_SIZE + 1` must be rejected by the
    /// pre-read metadata check.  Rejection via the post-read length check is
    /// exercised by the `/dev/zero`-style tests; here we want the fast path.
    #[test]
    fn test_read_wasm_bounded_boundary_reject() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("over.wasm");
        let content = vec![0u8; 1025];
        std::fs::write(&path, &content).unwrap();

        let err = read_wasm_file_bounded(&path, 1024).expect_err("1025-byte file must be rejected");
        match err {
            DownloadError::TooLarge { size, max } => {
                assert_eq!(size, 1025);
                assert_eq!(max, 1024);
            }
            other => panic!("expected TooLarge, got {:?}", other),
        }
    }

    /// Sparse file (created via `set_len`) larger than the limit must be
    /// rejected without allocating `metadata.len()` bytes.  We verify the
    /// error variant — not memory behaviour, which is hard to assert
    /// portably — because the whole point of the bounded read is that the
    /// allocation never happens.
    #[test]
    fn test_read_wasm_bounded_sparse_oversized() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sparse.wasm");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(10 * 1024 * 1024).unwrap(); // 10 MiB sparse

        let err = read_wasm_file_bounded(&path, 4096)
            .expect_err("sparse 10MiB must be rejected at 4KiB limit");
        assert!(matches!(err, DownloadError::TooLarge { .. }));
    }

    /// On Unix, a FIFO (named pipe) must be rejected as "not a regular file"
    /// rather than blocking on open or read.  Without `O_NONBLOCK`, opening
    /// a FIFO with no writer attached would block the caller forever; the
    /// helper sidesteps this by opening non-blocking and then validating the
    /// file type via `metadata.is_file()`.
    #[test]
    #[cfg(unix)]
    fn test_read_wasm_bounded_rejects_fifo() {
        let dir = tempdir().unwrap();
        let fifo_path = dir.path().join("wasm.fifo");

        let c_path = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        // SAFETY: `c_path` is a valid null-terminated C string for the lifetime
        // of the call; mkfifo takes a const pointer and does not retain it.
        let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) };
        if rc != 0 {
            eprintln!("mkfifo unavailable on this platform; skipping");
            return;
        }

        let err = read_wasm_file_bounded(&fifo_path, 1024)
            .expect_err("FIFO must be rejected rather than blocking");
        match err {
            DownloadError::IoError(e) => {
                assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
                assert!(
                    e.to_string().contains("not a regular file"),
                    "unexpected error text: {e}"
                );
            }
            other => panic!("expected IoError, got {:?}", other),
        }
    }

    /// Symlink pointing at an in-range regular file must succeed — the helper
    /// must not spuriously reject valid symlinks.  This is the "happy path"
    /// counterpart to the swap race below.
    #[test]
    #[cfg(unix)]
    fn test_read_wasm_bounded_follows_symlink_ok() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let target = dir.path().join("real.wasm");
        let link = dir.path().join("link.wasm");
        std::fs::write(&target, b"symlink target content").unwrap();
        symlink(&target, &link).unwrap();

        let bytes =
            read_wasm_file_bounded(&link, 1024).expect("symlink to small file must succeed");
        assert_eq!(bytes, b"symlink target content");
    }

    /// Symlink swap TOCTOU: after `open(link)` succeeds, the symlink target
    /// is replaced with an oversized file.  Because we operate on the
    /// already-opened file descriptor (not the path), the post-read size
    /// check still reflects the originally-opened inode.  This test
    /// documents that `open` commits us to the original inode and that a
    /// subsequent swap cannot trick the size check; the swap itself cannot
    /// inflate the bytes read through the old fd.
    #[test]
    #[cfg(unix)]
    fn test_read_wasm_bounded_symlink_swap_after_open() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let small = dir.path().join("small.wasm");
        let huge = dir.path().join("huge.wasm");
        let link = dir.path().join("link.wasm");

        std::fs::write(&small, b"small").unwrap();
        let huge_file = std::fs::File::create(&huge).unwrap();
        huge_file.set_len(10 * 1024 * 1024).unwrap(); // 10 MiB sparse
        symlink(&small, &link).unwrap();

        // Swap symlink target before the helper runs — this simulates the
        // attacker winning the race before we open at all.  The helper must
        // reject the oversized file via the metadata check.
        std::fs::remove_file(&link).unwrap();
        symlink(&huge, &link).unwrap();

        let err = read_wasm_file_bounded(&link, 4096)
            .expect_err("swapped symlink to oversized file must be rejected");
        assert!(matches!(err, DownloadError::TooLarge { .. }));
    }

    /// Non-existent path must surface as an I/O error, not a silent empty
    /// read.  This guards against the obvious failure mode of a missing
    /// cache file or manifest-listed WASM.
    #[test]
    fn test_read_wasm_bounded_missing_file() {
        let dir = tempdir().unwrap();
        let err = read_wasm_file_bounded(&dir.path().join("missing.wasm"), 1024)
            .expect_err("missing file must error");
        assert!(matches!(err, DownloadError::IoError(_)));
    }

    /// Directory path must be rejected rather than read as a file.  Without
    /// the regular-file check, `File::open` on a directory succeeds on some
    /// Unix platforms and the ensuing read would return `EISDIR`.
    #[test]
    fn test_read_wasm_bounded_rejects_directory() {
        let dir = tempdir().unwrap();
        let err = read_wasm_file_bounded(dir.path(), 1024).expect_err("directory must be rejected");
        match err {
            DownloadError::IoError(e) => {
                // Either our explicit "not a regular file" rejection or a
                // platform-level EISDIR from `read_to_end` — both are valid
                // rejections; we only need to ensure we do not silently
                // treat a directory as an empty WASM artefact.
                let msg = e.to_string();
                assert!(
                    msg.contains("not a regular file")
                        || e.kind() == std::io::ErrorKind::IsADirectory
                        || e.raw_os_error() == Some(21),
                    "unexpected error: {e}"
                );
            }
            other => panic!("expected IoError, got {:?}", other),
        }
    }

    /// End-to-end coverage of the cache-hit branch (L167 in the original
    /// code): after the first resolve populates the cache, swap the cached
    /// WASM for an oversized sparse file and confirm the resolver falls
    /// back to a re-download instead of OOM-ing or panicking.
    #[tokio::test]
    async fn test_resolve_cached_oversized_wasm_redownloads() {
        let mock_server = MockServer::start().await;
        let wasm_content = b"fake wasm content";
        let wasm_hash = HashVerifier::compute(wasm_content);

        let manifest = json!({
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "isolation_level": "global"
            },
            "wasm": [{
                "url": format!("{}/rule.wasm", mock_server.uri()),
                "hash": wasm_hash
            }]
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

        let fetcher = ManifestFetcher::new().allow_local(true);
        let downloader = WasmDownloader::new()
            .expect("Failed to create downloader")
            .allow_local(true);

        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path().to_path_buf());
        let resolver = PluginResolver::with_fetcher(fetcher)
            .expect("Failed to create resolver")
            .with_downloader(downloader)
            .with_cache(cache);

        let spec = github_spec_with_server_url(&mock_server.uri());

        let resolved1 = resolver.resolve(&spec).await.expect("First resolve failed");

        // Replace the cached WASM with a sparse file larger than DEFAULT_MAX_SIZE.
        // A naive `std::fs::read` would attempt to read 50 MiB + 1 bytes into
        // memory; `read_wasm_file_bounded` rejects it at the metadata check and
        // the resolver falls back to re-downloading the legitimate bytes.
        std::fs::remove_file(&resolved1.wasm_path).unwrap();
        let swapped = std::fs::File::create(&resolved1.wasm_path).unwrap();
        swapped
            .set_len(crate::downloader::DEFAULT_MAX_SIZE + 1)
            .unwrap();

        let resolved2 = resolver
            .resolve(&spec)
            .await
            .expect("Second resolve must fall back to re-download");

        // After the re-download, the cached bytes must be the legitimate
        // content (not the sparse oversized file).
        let final_bytes = std::fs::read(&resolved2.wasm_path).unwrap();
        assert_eq!(final_bytes, wasm_content);
    }
}
