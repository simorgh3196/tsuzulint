use crate::fetcher::PluginSource;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cache directory resolution failed")]
    DirResolutionFailed,
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

        match source {
            PluginSource::GitHub { owner, repo, .. } => {
                if is_safe(owner) && is_safe(repo) && is_safe(version) {
                    Some(self.cache_dir.join(owner).join(repo).join(version))
                } else {
                    None
                }
            }
            _ => None,
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
                "Caching is only supported for GitHub plugins",
            ))
        })?;

        std::fs::create_dir_all(&dir)?;

        let wasm_path = dir.join("rule.wasm");
        let manifest_path = dir.join("tsuzulint-rule.json");

        std::fs::write(&wasm_path, wasm_bytes)?;
        std::fs::write(&manifest_path, manifest_content)?;

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
}
