//! Cache entry types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tsuzulint_ast::Span;
use tsuzulint_plugin::Diagnostic;

/// A cached block of content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockCacheEntry {
    /// Hash of the block content.
    pub hash: String,

    /// Original span of the block.
    pub span: Span,

    /// Diagnostics associated with this block.
    pub diagnostics: Vec<Diagnostic>,
}

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

    /// Cached blocks for incremental updates.
    #[serde(default)]
    pub blocks: Vec<BlockCacheEntry>,

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
        blocks: Vec<BlockCacheEntry>,
    ) -> Self {
        Self {
            content_hash,
            config_hash,
            rule_versions,
            diagnostics,
            blocks,
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
            vec![],
        );

        assert!(!entry.is_valid("abc123", "config456", &versions2));
    }

    #[test]
    fn test_cache_entry_invalid_rule_count_mismatch() {
        let mut versions1 = HashMap::new();
        versions1.insert("rule1".to_string(), "1.0.0".to_string());

        let mut versions2 = HashMap::new();
        versions2.insert("rule1".to_string(), "1.0.0".to_string());
        versions2.insert("rule2".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions1,
            vec![],
            vec![],
        );

        // Different number of rules should invalidate
        assert!(!entry.is_valid("abc123", "config456", &versions2));
    }

    #[test]
    fn test_cache_entry_invalid_missing_rule() {
        let mut versions1 = HashMap::new();
        versions1.insert("rule1".to_string(), "1.0.0".to_string());

        let mut versions2 = HashMap::new();
        versions2.insert("rule2".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions1,
            vec![],
            vec![],
        );

        // Different rule names should invalidate
        assert!(!entry.is_valid("abc123", "config456", &versions2));
    }

    #[test]
    fn test_cache_entry_with_diagnostics() {
        use tsuzulint_ast::Span;
        let versions = HashMap::new();
        let diagnostics = vec![
            Diagnostic::new("rule1", "Error 1", Span::new(0, 5)),
            Diagnostic::new("rule2", "Error 2", Span::new(10, 15)),
        ];

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            diagnostics,
            vec![],
        );

        assert_eq!(entry.diagnostics.len(), 2);
        assert!(entry.is_valid("hash", "config", &versions));
    }

    #[test]
    fn test_cache_entry_with_blocks() {
        use tsuzulint_ast::Span;
        let versions = HashMap::new();
        let blocks = vec![
            BlockCacheEntry {
                hash: "block1".to_string(),
                span: Span::new(0, 10),
                diagnostics: vec![],
            },
            BlockCacheEntry {
                hash: "block2".to_string(),
                span: Span::new(11, 20),
                diagnostics: vec![Diagnostic::new("rule1", "Error", Span::new(12, 15))],
            },
        ];

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            blocks,
        );

        assert_eq!(entry.blocks.len(), 2);
        assert_eq!(entry.blocks[1].diagnostics.len(), 1);
    }

    #[test]
    fn test_cache_entry_created_at() {
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        // created_at should be a reasonable Unix timestamp (after 2020)
        assert!(entry.created_at > 1577836800);
    }

    #[test]
    fn test_cache_entry_serialization() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "abc123".to_string(),
            "config456".to_string(),
            versions,
            vec![],
            vec![],
        );

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("abc123"));
        assert!(json.contains("config456"));
        assert!(json.contains("rule1"));
    }

    #[test]
    fn test_cache_entry_deserialization() {
        let json = r#"{
            "content_hash": "hash123",
            "config_hash": "config456",
            "rule_versions": {},
            "diagnostics": [],
            "created_at": 1700000000
        }"#;

        let entry: CacheEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.content_hash, "hash123");
        assert_eq!(entry.config_hash, "config456");
        assert_eq!(entry.created_at, 1700000000);
        assert!(entry.blocks.is_empty());
    }

    #[test]
    fn test_cache_entry_clone() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());

        let original = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        let cloned = original.clone();

        assert_eq!(original.content_hash, cloned.content_hash);
        assert_eq!(original.config_hash, cloned.config_hash);
        assert_eq!(original.rule_versions, cloned.rule_versions);
    }

    #[test]
    fn test_cache_entry_empty_versions_valid() {
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        let empty_versions = HashMap::new();
        assert!(entry.is_valid("hash", "config", &empty_versions));
    }

    #[test]
    fn test_cache_entry_multiple_rules() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());
        versions.insert("rule2".to_string(), "2.0.0".to_string());
        versions.insert("rule3".to_string(), "3.0.0".to_string());

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        assert!(entry.is_valid("hash", "config", &versions));

        // Update one rule version
        let mut updated_versions = versions.clone();
        updated_versions.insert("rule2".to_string(), "2.1.0".to_string());

        assert!(!entry.is_valid("hash", "config", &updated_versions));
    }

    #[test]
    fn test_block_cache_entry_creation() {
        use tsuzulint_ast::Span;
        let block = BlockCacheEntry {
            hash: "block_hash".to_string(),
            span: Span::new(10, 20),
            diagnostics: vec![Diagnostic::new("rule1", "Error", Span::new(12, 15))],
        };

        assert_eq!(block.hash, "block_hash");
        assert_eq!(block.span.start, 10);
        assert_eq!(block.span.end, 20);
        assert_eq!(block.diagnostics.len(), 1);
    }

    #[test]
    fn test_cache_entry_is_valid_all_hashes_match() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "content_hash_123".to_string(),
            "config_hash_456".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        // All parameters match
        assert!(entry.is_valid("content_hash_123", "config_hash_456", &versions));
    }

    #[test]
    fn test_cache_entry_is_valid_content_mismatch() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "content_hash_123".to_string(),
            "config_hash_456".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        // Content hash mismatch
        assert!(!entry.is_valid("different_content_hash", "config_hash_456", &versions));
    }

    #[test]
    fn test_cache_entry_is_valid_config_mismatch() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "content_hash_123".to_string(),
            "config_hash_456".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        // Config hash mismatch
        assert!(!entry.is_valid("content_hash_123", "different_config_hash", &versions));
    }

    #[test]
    fn test_cache_entry_is_valid_rule_added() {
        let mut versions1 = HashMap::new();
        versions1.insert("rule1".to_string(), "1.0.0".to_string());

        let mut versions2 = HashMap::new();
        versions2.insert("rule1".to_string(), "1.0.0".to_string());
        versions2.insert("rule2".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions1,
            vec![],
            vec![],
        );

        // New rule added should invalidate
        assert!(!entry.is_valid("hash", "config", &versions2));
    }

    #[test]
    fn test_cache_entry_is_valid_rule_removed() {
        let mut versions1 = HashMap::new();
        versions1.insert("rule1".to_string(), "1.0.0".to_string());
        versions1.insert("rule2".to_string(), "1.0.0".to_string());

        let mut versions2 = HashMap::new();
        versions2.insert("rule1".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions1,
            vec![],
            vec![],
        );

        // Rule removed should invalidate
        assert!(!entry.is_valid("hash", "config", &versions2));
    }

    #[test]
    fn test_cache_entry_timestamp_is_recent() {
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        // Timestamp should be within last minute
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        assert!(entry.created_at <= now);
        assert!(entry.created_at >= now - 60);
    }

    #[test]
    fn test_cache_entry_with_multiple_diagnostics() {
        use tsuzulint_ast::Span;
        let diagnostics = vec![
            Diagnostic::new("rule1", "Error 1", Span::new(0, 5)),
            Diagnostic::new("rule2", "Error 2", Span::new(10, 15)),
            Diagnostic::new("rule3", "Error 3", Span::new(20, 25)),
        ];

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            diagnostics,
            vec![],
        );

        assert_eq!(entry.diagnostics.len(), 3);
        assert_eq!(entry.diagnostics[0].rule_id, "rule1");
        assert_eq!(entry.diagnostics[1].rule_id, "rule2");
        assert_eq!(entry.diagnostics[2].rule_id, "rule3");
    }

    #[test]
    fn test_cache_entry_with_multiple_blocks() {
        use tsuzulint_ast::Span;
        let blocks = vec![
            BlockCacheEntry {
                hash: "block1".to_string(),
                span: Span::new(0, 10),
                diagnostics: vec![],
            },
            BlockCacheEntry {
                hash: "block2".to_string(),
                span: Span::new(11, 20),
                diagnostics: vec![Diagnostic::new("rule1", "Error", Span::new(12, 15))],
            },
            BlockCacheEntry {
                hash: "block3".to_string(),
                span: Span::new(21, 30),
                diagnostics: vec![],
            },
        ];

        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            blocks,
        );

        assert_eq!(entry.blocks.len(), 3);
        assert_eq!(entry.blocks[0].hash, "block1");
        assert_eq!(entry.blocks[1].diagnostics.len(), 1);
        assert_eq!(entry.blocks[2].hash, "block3");
    }

    #[test]
    fn test_block_cache_entry_with_empty_diagnostics() {
        use tsuzulint_ast::Span;
        let block = BlockCacheEntry {
            hash: "block_hash".to_string(),
            span: Span::new(0, 10),
            diagnostics: vec![],
        };

        assert!(block.diagnostics.is_empty());
    }

    #[test]
    fn test_cache_entry_serialization_roundtrip() {
        let mut versions = HashMap::new();
        versions.insert("rule1".to_string(), "1.0.0".to_string());

        let entry = CacheEntry::new(
            "hash123".to_string(),
            "config456".to_string(),
            versions,
            vec![],
            vec![],
        );

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: CacheEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(entry.content_hash, deserialized.content_hash);
        assert_eq!(entry.config_hash, deserialized.config_hash);
        assert_eq!(entry.rule_versions, deserialized.rule_versions);
        assert_eq!(entry.created_at, deserialized.created_at);
    }

    #[test]
    fn test_cache_entry_with_special_characters_in_hashes() {
        let entry = CacheEntry::new(
            "hash-with-dashes_and_underscores.123".to_string(),
            "config/with/slashes".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: CacheEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(entry.content_hash, deserialized.content_hash);
        assert_eq!(entry.config_hash, deserialized.config_hash);
    }

    #[test]
    fn test_cache_entry_is_valid_empty_rule_versions() {
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        // Empty rule versions should match empty input
        assert!(entry.is_valid("hash", "config", &HashMap::new()));
    }

    #[test]
    fn test_cache_entry_is_valid_case_sensitive_hashes() {
        let versions = HashMap::new();

        let entry = CacheEntry::new(
            "Hash123".to_string(),
            "Config456".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        // Hashes should be case-sensitive
        assert!(entry.is_valid("Hash123", "Config456", &versions));
        assert!(!entry.is_valid("hash123", "Config456", &versions));
        assert!(!entry.is_valid("Hash123", "config456", &versions));
    }
}