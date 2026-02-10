use crate::fetcher::PluginSource;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("Permission denied: {0}")]
    PermissionDenied(std::io::Error),
    #[error("Cache directory resolution failed")]
    DirResolutionFailed,
}

impl From<std::io::Error> for CacheError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            CacheError::PermissionDenied(err)
        } else {
            CacheError::Io(err)
        }
    }
}

pub struct CachedPlugin {
    pub wasm_path: PathBuf,
    pub manifest_path: PathBuf,
}

pub struct PluginCache {
    cache_dir: PathBuf,
}

impl PluginCache {
    /// Create a new plugin cache with default location.
    pub fn new() -> Result<Self, CacheError> {
        let base_dir = dirs::cache_dir().ok_or(CacheError::DirResolutionFailed)?;
        let cache_dir = base_dir.join("tsuzulint").join("plugins");
        Ok(Self { cache_dir })
    }

    /// Create a new plugin cache with a specific root directory (for testing).
    pub fn with_dir(path: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: path.into(),
        }
    }

    fn get_path(&self, source: &PluginSource, version: &str) -> Option<PathBuf> {
        // Validation helper: ensure segment is a single normal component
        let is_safe = |s: &str| {
            let path = std::path::Path::new(s);
            let mut components = path.components();
            match (components.next(), components.next()) {
                (Some(std::path::Component::Normal(c)), None) => c == s,
                _ => false,
            }
        };

        if !is_safe(version) {
            return None;
        }

        match source {
            PluginSource::GitHub { owner, repo, .. } => {
                if is_safe(owner) && is_safe(repo) {
                    Some(self.cache_dir.join(owner).join(repo).join(version))
                } else {
                    None
                }
            }
            PluginSource::Url(url) => {
                // For URL, we use a hash of the URL as the directory name
                // to avoid issues with special characters in URLs.
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(url.as_bytes());
                let result = hasher.finalize();
                let hash_hex = hex::encode(result);

                Some(self.cache_dir.join("url").join(hash_hex).join(version))
            }
            PluginSource::Path(_) => {
                // Local paths are not cached
                None
            }
        }
    }

    /// Get a plugin from the cache.
    pub fn get(&self, source: &PluginSource, version: &str) -> Option<CachedPlugin> {
        let dir = self.get_path(source, version)?;
        let wasm_path = dir.join("rule.wasm");
        let manifest_path = dir.join("tsuzulint-rule.json");

        if wasm_path.exists() && manifest_path.exists() {
            Some(CachedPlugin {
                wasm_path,
                manifest_path,
            })
        } else {
            None
        }
    }

    /// Store a plugin in the cache.
    pub fn store(
        &self,
        source: &PluginSource,
        version: &str,
        wasm_bytes: &[u8],
        manifest_content: &str,
    ) -> Result<CachedPlugin, CacheError> {
        let dir = self.get_path(source, version).ok_or_else(|| {
            CacheError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Caching is not supported for this plugin source",
            ))
        })?;

        std::fs::create_dir_all(&dir)?;

        let wasm_path = dir.join("rule.wasm");
        let manifest_path = dir.join("tsuzulint-rule.json");

        std::fs::write(&wasm_path, wasm_bytes)?;

        // If artifacts.wasm is a URL, we rewrite it to "rule.wasm" for the cached manifest
        // This ensures tsuzulint_core doesn't need to know about URLs or fallback logic
        let mut manifest_to_write = manifest_content.to_string();
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(manifest_content) {
            let is_remote_url = json["artifacts"]["wasm"]
                .as_str()
                .map(|s| s.starts_with("http://") || s.starts_with("https://"))
                .unwrap_or(false);

            if is_remote_url {
                if let Some(wasm) = json.get_mut("artifacts").and_then(|a| a.get_mut("wasm")) {
                    // Rewrite to local file name consistent with cache layout
                    *wasm = serde_json::Value::String("rule.wasm".to_string());
                    if let Ok(rewritten) = serde_json::to_string_pretty(&json) {
                        manifest_to_write = rewritten;
                    }
                }
            }
        }

        std::fs::write(&manifest_path, manifest_to_write)?;

        Ok(CachedPlugin {
            wasm_path,
            manifest_path,
        })
    }

    /// Remove a specific plugin from the cache.
    pub fn remove(&self, source: &PluginSource, version: &str) -> Result<(), CacheError> {
        let dir = self.get_path(source, version).ok_or_else(|| {
            CacheError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Caching is only supported for GitHub plugins",
            ))
        })?;

        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }

        Ok(())
    }

    /// Clear the entire plugin cache.
    pub fn clear(&self) -> Result<(), CacheError> {
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn should_return_none_when_cache_is_empty() {
        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());
        let source = PluginSource::github("owner", "repo");

        let result = cache.get(&source, "1.0.0");
        assert!(result.is_none());
    }

    #[test]
    fn should_store_and_retrieve_plugin() {
        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());
        let source = PluginSource::github("owner", "repo");
        let version = "1.0.0";
        let wasm_bytes = b"dummy wasm content";
        let manifest_content = r#"{"name": "test-rule", "version": "1.0.0"}"#;

        // Store
        let stored = cache
            .store(&source, version, wasm_bytes, manifest_content)
            .unwrap();

        // Verify return value
        assert!(stored.wasm_path.exists());
        assert!(stored.manifest_path.exists());
        assert_eq!(fs::read(&stored.wasm_path).unwrap(), wasm_bytes);
        assert_eq!(
            fs::read_to_string(&stored.manifest_path).unwrap(),
            manifest_content
        );

        // Retrieve
        let retrieved = cache.get(&source, version).unwrap();
        assert_eq!(retrieved.wasm_path, stored.wasm_path);
        assert_eq!(retrieved.manifest_path, stored.manifest_path);
    }

    #[test]
    fn should_remove_specific_plugin_version() {
        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());
        let source = PluginSource::github("owner", "repo");
        let v1 = "1.0.0";
        let v2 = "2.0.0";

        let _ = cache.store(&source, v1, b"v1", "{}").unwrap();
        let _ = cache.store(&source, v2, b"v2", "{}").unwrap();

        // Remove v1
        cache.remove(&source, v1).unwrap();

        // v1 should be gone, v2 should remain
        assert!(cache.get(&source, v1).is_none());
        assert!(cache.get(&source, v2).is_some());
    }

    #[test]
    fn should_clear_entire_cache() {
        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());
        let source1 = PluginSource::github("owner1", "repo1");
        let source2 = PluginSource::github("owner2", "repo2");

        let _ = cache.store(&source1, "1.0", b"c1", "{}").unwrap();
        let _ = cache.store(&source2, "1.0", b"c2", "{}").unwrap();

        // Clear
        cache.clear().unwrap();

        // Both should be gone
        assert!(cache.get(&source1, "1.0").is_none());
        assert!(cache.get(&source2, "1.0").is_none());

        // Root dir should be empty or gone
        assert!(!temp_dir.path().join("owner1").exists());
        assert!(!temp_dir.path().join("owner2").exists());
    }

    #[test]
    fn should_prevent_path_traversal() {
        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());

        let source = PluginSource::github("owner", "repo");
        let malicious_version = "../../../etc/passwd";

        // Try to store with malicious version
        let result = cache.store(&source, malicious_version, b"evil", "{}");

        // Should either fail with error or be sanitized
        // For this test, we expect it to be sanitized and NOT write outside the cache dir
        if let Ok(stored) = result {
            assert!(stored.wasm_path.starts_with(temp_dir.path()));
            assert!(!stored.wasm_path.to_string_lossy().contains(".."));
        } else {
            // Or explicitly return an error for invalid paths
            // assert!(matches!(result.unwrap_err(), CacheError::InvalidPath(_)));
        }
    }

    #[test]
    #[cfg(unix)]
    fn should_report_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());
        let source = PluginSource::github("owner", "repo");

        // Create the directory with read-only permissions
        let cache_path = temp_dir.path().join("owner/repo/1.0.0");
        fs::create_dir_all(&cache_path).unwrap();

        // Remove write permissions from the directory so we can't create files inside it
        let mut perms = fs::metadata(&cache_path).unwrap().permissions();
        perms.set_mode(0o500); // Read/Execute only, no Write
        fs::set_permissions(&cache_path, perms).unwrap();

        let result = cache.store(&source, "1.0.0", b"test", "{}");

        match result {
            Err(CacheError::PermissionDenied(_)) => {
                // Success: PermissionDenied is the expected error
            }
            Err(e) if e.to_string().to_lowercase().contains("permission denied") => {
                // If run as root or on some file systems, permission might not be denied
                // But if it IS denied, it MUST be PermissionDenied variant, not generic Io
                panic!(
                    "Got IO error with 'Permission denied' message but not PermissionDenied variant: {:?}",
                    e
                );
            }
            Ok(_) => {
                // Maybe running as root?
            }
            Err(e) => {
                // Any other error is unexpected for this test scenario
                panic!("Unexpected error: {:?}", e);
            }
        }
    }

    #[test]
    fn should_rewrite_url_artifact_to_local_path() {
        let temp_dir = tempdir().unwrap();
        let cache = PluginCache::with_dir(temp_dir.path());
        let source = PluginSource::Url("https://example.com/manifest.json".to_string());

        // Manifest with URL artifact
        let manifest_content = r#"{
            "rule": { "name": "url-rule", "version": "1.0.0" },
            "artifacts": {
                "wasm": "https://example.com/rule.wasm",
                "sha256": "hash"
            }
        }"#;

        let stored = cache
            .store(&source, "1.0.0", b"wasm", manifest_content)
            .unwrap();

        // Verify manifest content on disk
        let saved_content = fs::read_to_string(&stored.manifest_path).unwrap();
        let saved_json: serde_json::Value = serde_json::from_str(&saved_content).unwrap();

        let wasm_path = saved_json["artifacts"]["wasm"].as_str().unwrap();
        assert_eq!(wasm_path, "rule.wasm");
    }
}
