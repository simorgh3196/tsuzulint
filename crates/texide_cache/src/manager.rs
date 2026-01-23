//! Cache manager for file-level caching.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info};

use crate::{CacheEntry, CacheError};

/// Manages the lint cache for all files.
pub struct CacheManager {
    /// Directory where cache files are stored.
    cache_dir: PathBuf,
    /// In-memory cache entries.
    entries: HashMap<PathBuf, CacheEntry>,
    /// Whether cache is enabled.
    enabled: bool,
}

impl CacheManager {
    /// Creates a new cache manager.
    ///
    /// # Arguments
    ///
    /// * `cache_dir` - Directory to store cache files
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            entries: HashMap::new(),
            enabled: true,
        }
    }

    /// Disables caching.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Enables caching.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Returns whether caching is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Computes the BLAKE3 hash of content.
    pub fn hash_content(content: &str) -> String {
        blake3::hash(content.as_bytes()).to_hex().to_string()
    }

    /// Gets a cached entry for a file.
    pub fn get(&self, path: &Path) -> Option<&CacheEntry> {
        if !self.enabled {
            return None;
        }
        self.entries.get(path)
    }

    /// Checks if a file's cache is valid.
    ///
    /// # Arguments
    ///
    /// * `path` - File path
    /// * `content_hash` - Hash of current file content
    /// * `config_hash` - Hash of current configuration
    /// * `rule_versions` - Current rule versions
    pub fn is_valid(
        &self,
        path: &Path,
        content_hash: &str,
        config_hash: &str,
        rule_versions: &HashMap<String, String>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        match self.entries.get(path) {
            Some(entry) => entry.is_valid(content_hash, config_hash, rule_versions),
            None => false,
        }
    }

    /// Stores a cache entry for a file.
    pub fn set(&mut self, path: PathBuf, entry: CacheEntry) {
        if self.enabled {
            self.entries.insert(path, entry);
        }
    }

    /// Removes a cache entry.
    pub fn remove(&mut self, path: &Path) {
        self.entries.remove(path);
    }

    /// Clears all cache entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Loads cache from disk.
    pub fn load(&mut self) -> Result<(), CacheError> {
        if !self.enabled {
            return Ok(());
        }

        let cache_file = self.cache_dir.join("cache.json");

        if !cache_file.exists() {
            debug!("No cache file found at {}", cache_file.display());
            return Ok(());
        }

        let content = fs::read_to_string(&cache_file)?;
        let entries: HashMap<PathBuf, CacheEntry> =
            serde_json::from_str(&content).map_err(|e| CacheError::corrupted(e.to_string()))?;

        info!("Loaded {} cache entries", entries.len());
        self.entries = entries;

        Ok(())
    }

    /// Saves cache to disk.
    pub fn save(&self) -> Result<(), CacheError> {
        if !self.enabled {
            return Ok(());
        }

        // Ensure cache directory exists
        fs::create_dir_all(&self.cache_dir)?;

        let cache_file = self.cache_dir.join("cache.json");
        let content = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| CacheError::Serialization(e.to_string()))?;

        fs::write(&cache_file, content)?;

        info!(
            "Saved {} cache entries to {}",
            self.entries.len(),
            cache_file.display()
        );

        Ok(())
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new(".texide-cache")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_manager_new() {
        let manager = CacheManager::new("/tmp/test-cache");
        assert!(manager.is_enabled());
        assert!(manager.is_empty());
    }

    #[test]
    fn test_cache_manager_disable() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        manager.disable();
        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_cache_manager_set_get() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");
        let entry = CacheEntry::new(
            "hash123".to_string(),
            "config456".to_string(),
            HashMap::new(),
            vec![],
        );

        manager.set(path.clone(), entry);

        assert!(manager.get(&path).is_some());
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_cache_manager_is_valid() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");
        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "hash123".to_string(),
            "config456".to_string(),
            versions.clone(),
            vec![],
        );

        manager.set(path.clone(), entry);

        assert!(manager.is_valid(&path, "hash123", "config456", &versions));
        assert!(!manager.is_valid(&path, "different", "config456", &versions));
    }

    #[test]
    fn test_hash_content() {
        let hash1 = CacheManager::hash_content("hello");
        let hash2 = CacheManager::hash_content("hello");
        let hash3 = CacheManager::hash_content("world");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}
