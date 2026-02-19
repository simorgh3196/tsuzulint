//! Cache manager for file-level caching.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info};
use tsuzulint_ast::Span;
use tsuzulint_plugin::Diagnostic;

use crate::{CacheEntry, CacheError, entry::BlockCacheEntry};

/// Manages the lint cache for all files.
pub struct CacheManager {
    /// Directory where cache files are stored.
    cache_dir: PathBuf,
    /// In-memory cache entries.
    entries: HashMap<String, CacheEntry>,
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
        let key = path.to_string_lossy().to_string();
        self.entries.get(&key)
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

        let key = path.to_string_lossy().to_string();
        match self.entries.get(&key) {
            Some(entry) => entry.is_valid(content_hash, config_hash, rule_versions),
            None => false,
        }
    }

    /// Reconciles cached diagnostics with current blocks.
    ///
    /// This function tries to reuse cached diagnostics for blocks that haven't changed.
    /// It returns a list of diagnostics from unchanged blocks, but with their spans shifted.
    ///
    /// # Arguments
    ///
    /// * `path` - File path
    /// * `current_blocks` - Current blocks in the file
    /// * `config_hash` - Hash of current configuration
    /// * `rule_versions` - Current rule versions
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - `Vec<Diagnostic>`: Diagnostics from unchanged blocks (shifted)
    /// - `Vec<bool>`: A boolean mask indicating which current blocks matched (true = reused, false = changed/new)
    pub fn reconcile_blocks(
        &self,
        path: &Path,
        current_blocks: &[BlockCacheEntry],
        config_hash: &str,
        rule_versions: &HashMap<String, String>,
    ) -> (Vec<Diagnostic>, Vec<bool>) {
        let mut reused_diagnostics = Vec::new();
        let mut matched_mask = vec![false; current_blocks.len()];

        if !self.enabled {
            return (reused_diagnostics, matched_mask);
        }

        let key = path.to_string_lossy().to_string();
        let cached_entry = match self.entries.get(&key) {
            Some(entry) => entry,
            None => return (reused_diagnostics, matched_mask),
        };

        // Check if config/rules are compatible
        if cached_entry.config_hash != config_hash
            || cached_entry.rule_versions.len() != rule_versions.len()
        {
            return (reused_diagnostics, matched_mask);
        }

        for (name, version) in &cached_entry.rule_versions {
            if rule_versions.get(name) != Some(version) {
                return (reused_diagnostics, matched_mask);
            }
        }

        // Simple reconciliation algorithm:
        // Match blocks by hash. If hash matches, we assume content is same.
        // We need to account for position shifts.

        // Map of hash -> Vec<BlockCacheEntry> from cache
        // We use a Vec because multiple blocks might have same content (and thus same hash)
        let mut cached_blocks_map: HashMap<String, Vec<&BlockCacheEntry>> = HashMap::new();
        for block in &cached_entry.blocks {
            cached_blocks_map
                .entry(block.hash.clone())
                .or_default()
                .push(block);
        }

        // Iterate current blocks and try to find match
        for (i, current_block) in current_blocks.iter().enumerate() {
            if let Some(candidates) = cached_blocks_map.get_mut(&current_block.hash)
                && let Some(best_match_idx) = Self::find_best_match(current_block, candidates)
            {
                // Optimization: swap_remove is O(1) while remove is O(N).
                // Although swap_remove changes candidate order (affecting tie-breaking in
                // find_best_match), correctness is preserved: all candidates share the same
                // hash (identical content), so their relative diagnostic positions are also
                // identical and any matched candidate produces the same shifted diagnostics.
                let matched_block = candidates.swap_remove(best_match_idx);
                matched_mask[i] = true;

                // Calculate offset shift
                let shift = (current_block.span.start as i64) - (matched_block.span.start as i64);

                // Add diagnostics from matched block, shifted
                for diag in &matched_block.diagnostics {
                    let mut new_diag = diag.clone();
                    // Shift span
                    let new_start = (diag.span.start as i64 + shift) as u32;
                    let new_end = (diag.span.end as i64 + shift) as u32;
                    new_diag.span = Span::new(new_start, new_end);

                    // Shift fix if exists
                    if let Some(fix) = &mut new_diag.fix {
                        let fix_start = (fix.span.start as i64 + shift) as u32;
                        let fix_end = (fix.span.end as i64 + shift) as u32;
                        fix.span = Span::new(fix_start, fix_end);
                    }

                    // Note: Location (line/col) would need recalculation, but it's derived from source + span.
                    // We clear it so it gets recomputed if needed, or we rely on Span.
                    new_diag.loc = None;

                    reused_diagnostics.push(new_diag);
                }
            }
        }

        (reused_diagnostics, matched_mask)
    }

    /// Finds the best match among candidates for a current block.
    /// Selects the candidate whose start position is closest to the current block's start.
    fn find_best_match(
        current_block: &BlockCacheEntry,
        candidates: &[&BlockCacheEntry],
    ) -> Option<usize> {
        candidates
            .iter()
            .enumerate()
            .min_by_key(|(_, candidate)| {
                (current_block.span.start as i64 - candidate.span.start as i64).abs()
            })
            .map(|(index, _)| index)
    }

    /// Stores a cache entry for a file.
    pub fn set(&mut self, path: PathBuf, entry: CacheEntry) {
        if self.enabled {
            let key = path.to_string_lossy().to_string();
            self.entries.insert(key, entry);
        }
    }

    /// Removes a cache entry.
    pub fn remove(&mut self, path: &Path) {
        let key = path.to_string_lossy().to_string();
        self.entries.remove(&key);
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

        let cache_file = self.cache_dir.join("cache.rkyv");

        if !cache_file.exists() {
            debug!("No cache file found at {}", cache_file.display());
            return Ok(());
        }

        let content = fs::read(&cache_file)?;
        let entries: HashMap<String, CacheEntry> =
            rkyv::from_bytes::<_, rkyv::rancor::Error>(&content)
                .map_err(|e| CacheError::corrupted(e.to_string()))?;

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

        let cache_file = self.cache_dir.join("cache.rkyv");
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&self.entries)
            .map_err(|e| CacheError::Serialization(e.to_string()))?;

        fs::write(&cache_file, bytes)?;

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
        Self::new(".tsuzulint-cache")
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

    #[test]
    fn test_cache_manager_enable() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        manager.disable();
        assert!(!manager.is_enabled());

        manager.enable();
        assert!(manager.is_enabled());
    }

    #[test]
    fn test_cache_manager_remove() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        manager.set(path.clone(), entry);
        assert_eq!(manager.len(), 1);

        manager.remove(&path);
        assert!(manager.is_empty());
    }

    #[test]
    fn test_cache_manager_clear() {
        let mut manager = CacheManager::new("/tmp/test-cache");

        for i in 0..5 {
            let path = PathBuf::from(format!("/test/file{}.md", i));
            let entry = CacheEntry::new(
                format!("hash{}", i),
                "config".to_string(),
                HashMap::new(),
                vec![],
                vec![],
            );
            manager.set(path, entry);
        }

        assert_eq!(manager.len(), 5);

        manager.clear();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_cache_manager_get_when_disabled() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        manager.set(path.clone(), entry);
        manager.disable();

        // get should return None when disabled
        assert!(manager.get(&path).is_none());
    }

    #[test]
    fn test_cache_manager_is_valid_when_disabled() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");
        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            vec![],
        );

        manager.set(path.clone(), entry);
        manager.disable();

        // is_valid should return false when disabled
        assert!(!manager.is_valid(&path, "hash", "config", &versions));
    }

    #[test]
    fn test_cache_manager_set_when_disabled() {
        let mut manager = CacheManager::new("/tmp/test-cache");
        manager.disable();

        let path = PathBuf::from("/test/file.md");
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            HashMap::new(),
            vec![],
            vec![],
        );

        manager.set(path, entry);

        // Entry should not be stored when cache is disabled
        assert!(manager.is_empty());
    }

    #[test]
    fn test_cache_manager_is_valid_missing_entry() {
        let manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/nonexistent/file.md");
        let versions = HashMap::new();

        assert!(!manager.is_valid(&path, "hash", "config", &versions));
    }

    #[test]
    fn test_cache_manager_default() {
        let manager = CacheManager::default();
        assert!(manager.is_enabled());
        assert!(manager.is_empty());
    }

    #[test]
    fn test_hash_content_empty() {
        let hash = CacheManager::hash_content("");
        // Empty string should still produce a valid hash
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // BLAKE3 produces 256-bit (64 hex chars) hash
    }

    #[test]
    fn test_hash_content_unicode() {
        let hash1 = CacheManager::hash_content("日本語");
        let hash2 = CacheManager::hash_content("日本語");
        let hash3 = CacheManager::hash_content("中文");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_cache_manager_multiple_files() {
        let mut manager = CacheManager::new("/tmp/test-cache");

        let files = vec![
            ("/path/a.md", "hash_a"),
            ("/path/b.md", "hash_b"),
            ("/path/c.txt", "hash_c"),
        ];

        for (path, hash) in &files {
            let entry = CacheEntry::new(
                hash.to_string(),
                "config".to_string(),
                HashMap::new(),
                vec![],
                vec![],
            );
            manager.set(PathBuf::from(path), entry);
        }

        assert_eq!(manager.len(), 3);

        let versions = HashMap::new();
        assert!(manager.is_valid(&PathBuf::from("/path/a.md"), "hash_a", "config", &versions));
        assert!(manager.is_valid(&PathBuf::from("/path/b.md"), "hash_b", "config", &versions));
        assert!(manager.is_valid(&PathBuf::from("/path/c.txt"), "hash_c", "config", &versions));
    }

    #[test]
    fn test_reconcile_blocks_with_multiple_candidates_and_order() {
        use tsuzulint_ast::Span;
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");

        // Create 3 identical blocks at different positions in the cache
        // Hash "A" at 0-10, 20-30, 40-50
        let block1 = BlockCacheEntry {
            hash: "A".to_string(),
            span: Span::new(0, 10),
            diagnostics: vec![Diagnostic::new("rule1", "Err1", Span::new(1, 5))],
        };
        let block2 = BlockCacheEntry {
            hash: "A".to_string(),
            span: Span::new(20, 30),
            diagnostics: vec![Diagnostic::new("rule1", "Err2", Span::new(21, 25))],
        };
        let block3 = BlockCacheEntry {
            hash: "A".to_string(),
            span: Span::new(40, 50),
            diagnostics: vec![Diagnostic::new("rule1", "Err3", Span::new(41, 45))],
        };

        let cached_blocks = vec![block1, block2, block3];
        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            cached_blocks,
        );
        manager.set(path.clone(), entry);

        // Current blocks:
        // 1. "A" at 0-10 (should match cached block1)
        // 2. "A" at 40-50 (should match cached block3)
        // 3. "A" at 20-30 (should match cached block2)
        // Note: We process them in an order that might trigger swap_remove behavior if we are not careful,
        // but here we just want to ensure that "best match" (closest distance) is respected.

        // Let's mix up the order in current_blocks to test robustness
        let current_blocks = vec![
            BlockCacheEntry {
                hash: "A".to_string(),
                span: Span::new(0, 10), // Matches cached block1 (dist 0)
                diagnostics: vec![],
            },
            BlockCacheEntry {
                hash: "A".to_string(),
                span: Span::new(40, 50), // Matches cached block3 (dist 0)
                diagnostics: vec![],
            },
            BlockCacheEntry {
                hash: "A".to_string(),
                span: Span::new(20, 30), // Matches cached block2 (dist 0)
                diagnostics: vec![],
            },
        ];

        let (diagnostics, matched) =
            manager.reconcile_blocks(&path, &current_blocks, "config", &versions);

        // All 3 should match
        assert!(matched.iter().all(|&m| m));
        assert_eq!(diagnostics.len(), 3);

        // Verify that we got the correct diagnostics back (meaning correct blocks were matched)
        // Since we didn't shift positions (current == cached), spans should be identical to cached diagnostics.
        let messages: Vec<String> = diagnostics.iter().map(|d| d.message.clone()).collect();
        assert!(messages.contains(&"Err1".to_string()));
        assert!(messages.contains(&"Err2".to_string()));
        assert!(messages.contains(&"Err3".to_string()));
    }

    #[test]
    fn test_reconcile_blocks_span_shift() {
        use tsuzulint_ast::Span;
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");

        // Cached block "B" at 0-10 with diagnostic at 2-5
        let block = BlockCacheEntry {
            hash: "B".to_string(),
            span: Span::new(0, 10),
            diagnostics: vec![Diagnostic::new("rule1", "Err1", Span::new(2, 5))],
        };

        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            vec![block],
        );
        manager.set(path.clone(), entry);

        // Current block "B" at 20-30 (shifted by +20)
        let current_blocks = vec![BlockCacheEntry {
            hash: "B".to_string(),
            span: Span::new(20, 30),
            diagnostics: vec![],
        }];

        let (diagnostics, matched) =
            manager.reconcile_blocks(&path, &current_blocks, "config", &versions);

        // Assert matched
        assert_eq!(matched, vec![true]);
        assert_eq!(diagnostics.len(), 1);

        // Assert diagnostic span is shifted: (2+20, 5+20) = (22, 25)
        assert_eq!(diagnostics[0].span, Span::new(22, 25));
        assert_eq!(diagnostics[0].message, "Err1");
    }

    #[test]
    fn test_reconcile_blocks_tie_breaker() {
        use tsuzulint_ast::Span;
        let mut manager = CacheManager::new("/tmp/test-cache");
        let path = PathBuf::from("/test/file.md");

        // Two identical cached blocks with same hash "C"
        // Block 1: 5-10
        // Block 2: 15-20
        let block1 = BlockCacheEntry {
            hash: "C".to_string(),
            span: Span::new(5, 10),
            diagnostics: vec![Diagnostic::new("rule1", "Err1", Span::new(6, 8))],
        };
        let block2 = BlockCacheEntry {
            hash: "C".to_string(),
            span: Span::new(15, 20),
            diagnostics: vec![Diagnostic::new("rule1", "Err2", Span::new(16, 18))],
        };

        let versions = HashMap::new();
        let entry = CacheEntry::new(
            "hash".to_string(),
            "config".to_string(),
            versions.clone(),
            vec![],
            vec![block1, block2],
        );
        manager.set(path.clone(), entry);

        // Current block "C" at 10-15
        // Distance to block1 (5-10): |10-5| = 5
        // Distance to block2 (15-20): |10-15| = 5
        // This is a tie-breaker case for find_best_match.
        // It should pick one (the first one) and the other should remain unmatched.
        let current_blocks = vec![BlockCacheEntry {
            hash: "C".to_string(),
            span: Span::new(10, 15),
            diagnostics: vec![],
        }];

        let (diagnostics, matched) =
            manager.reconcile_blocks(&path, &current_blocks, "config", &versions);

        // Exactly one should match
        assert_eq!(matched, vec![true]);
        assert_eq!(diagnostics.len(), 1);

        // If block1 was picked (start 5, current 10, shift +5):
        // Span (6,8) -> (11,13)
        // If block2 was picked (start 15, current 10, shift -5):
        // Span (16,18) -> (11,13)
        // In both cases, the span should be (11,13) because they are identical blocks.
        assert_eq!(diagnostics[0].span, Span::new(11, 13));
    }
}
