//! Cache entry types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use texide_plugin::Diagnostic;

/// A cache entry for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Hash of the file content.
    pub content_hash: String,

    /// Hash of the configuration used.
    pub config_hash: String,

    /// Versions of rules used for this cache entry.
    pub rule_versions: HashMap<String, String>,

    /// Cached diagnostics.
    pub diagnostics: Vec<Diagnostic>,

    /// Timestamp when this entry was created.
    pub created_at: u64,
}

impl CacheEntry {
    /// Creates a new cache entry.
    pub fn new(
        content_hash: String,
        config_hash: String,
        rule_versions: HashMap<String, String>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            content_hash,
            config_hash,
            rule_versions,
            diagnostics,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    /// Checks if this cache entry is valid for the given hashes and versions.
    pub fn is_valid(
        &self,
        content_hash: &str,
        config_hash: &str,
        rule_versions: &HashMap<String, String>,
    ) -> bool {
        // Check content hash
        if self.content_hash != content_hash {
            return false;
        }

        // Check config hash
        if self.config_hash != config_hash {
            return false;
        }

        // Check rule versions
        if self.rule_versions.len() != rule_versions.len() {
            return false;
        }

        for (name, version) in &self.rule_versions {
            if rule_versions.get(name) != Some(version) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_valid() {
        let mut versions = HashMap::new();
        versions.insert("no-todo".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions.clone(),
            vec![],
        );

        assert!(entry.is_valid("abc123", "config456", &versions));
    }

    #[test]
    fn test_cache_entry_invalid_content() {
        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions.clone(),
            vec![],
        );

        assert!(!entry.is_valid("different", "config456", &versions));
    }

    #[test]
    fn test_cache_entry_invalid_config() {
        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions.clone(),
            vec![],
        );

        assert!(!entry.is_valid("abc123", "different", &versions));
    }

    #[test]
    fn test_cache_entry_invalid_rule_version() {
        let mut versions1 = HashMap::new();
        versions1.insert("no-todo".to_string(), "1.0.0".to_string());

        let mut versions2 = HashMap::new();
        versions2.insert("no-todo".to_string(), "2.0.0".to_string());

        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions1,
            vec![],
        );

        assert!(!entry.is_valid("abc123", "config456", &versions2));
    }
}
