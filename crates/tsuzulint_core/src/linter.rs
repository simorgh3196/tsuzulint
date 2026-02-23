//! Core linter engine.
//!
//! This module provides the main `Linter` struct that orchestrates file discovery,
//! parsing, rule execution, and caching. The actual implementation is split into
//! focused sub-modules:
//!
//! - [`rule_loader`]: Plugin/rule loading logic
//! - [`file_linter`]: Single file linting logic
//! - [`block_extractor`]: Block extraction for incremental caching
//! - [`ignore_range`]: Code block ignore range extraction
//! - [`diagnostic_dist`]: Diagnostic distribution to blocks
//! - [`manifest_resolver`]: Secure manifest path resolution
//! - [`parallel_linter`]: Parallel file linting logic

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use tsuzulint_cache::CacheManager;
use tsuzulint_text::Tokenizer;

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use tsuzulint_plugin::PluginHost;

use crate::config::LinterConfig;
use crate::error::LinterError;
use crate::file_linter::lint_content as lint_content_internal;
use crate::file_linter::lint_file_internal;
use crate::parallel_linter::lint_files as lint_files_parallel;
use crate::result::LintResult;
use crate::rule_loader::{create_plugin_host, load_configured_rules};

pub use crate::parallel_linter::LintFilesResult;

/// The core linter engine.
///
/// Orchestrates file discovery, parsing, rule execution, and caching.
pub struct Linter {
    tokenizer: Arc<Tokenizer>,
    config: LinterConfig,
    config_hash: String,
    plugin_host: Mutex<PluginHost>,
    cache: Mutex<CacheManager>,
    dynamic_rules: Mutex<Vec<PathBuf>>,
    include_globs: Option<GlobSet>,
    exclude_globs: Option<GlobSet>,
}

impl Linter {
    pub fn new(config: LinterConfig) -> Result<Self, LinterError> {
        let cache_dir = PathBuf::from(config.cache.path());
        let mut cache = CacheManager::new(cache_dir);

        if !config.cache.is_enabled() {
            cache.disable();
        }

        if let Err(e) = cache.load() {
            warn!("Failed to load cache: {}", e);
        }

        let tokenizer = Arc::new(Tokenizer::new().map_err(|e| {
            LinterError::Internal(format!("Failed to initialize tokenizer: {}", e))
        })?);

        let include_globs = Self::build_globset(&config.include)?;
        let exclude_globs = Self::build_globset(&config.exclude)?;

        let mut host = PluginHost::new();
        load_configured_rules(&config, &mut host);

        let config_hash = config.hash()?;

        Ok(Self {
            tokenizer,
            config,
            config_hash,
            plugin_host: Mutex::new(host),
            cache: Mutex::new(cache),
            dynamic_rules: Mutex::new(Vec::new()),
            include_globs,
            exclude_globs,
        })
    }

    fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>, LinterError> {
        if patterns.is_empty() {
            return Ok(None);
        }

        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = Glob::new(pattern)
                .map_err(|e| LinterError::config(format!("Invalid glob pattern: {}", e)))?;
            builder.add(glob);
        }

        let globset = builder
            .build()
            .map_err(|e| LinterError::config(format!("Failed to build globset: {}", e)))?;

        Ok(Some(globset))
    }

    pub fn load_rule(&self, path: impl AsRef<Path>) -> Result<(), LinterError> {
        let path_buf = path.as_ref().to_path_buf();

        {
            let mut host = self
                .plugin_host
                .lock()
                .map_err(|_| LinterError::Internal("Plugin host mutex poisoned".to_string()))?;
            host.load_rule(&path_buf)?;
        }

        {
            let mut list = self
                .dynamic_rules
                .lock()
                .map_err(|_| LinterError::Internal("Dynamic rules mutex poisoned".to_string()))?;
            list.push(path_buf);
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn lint_file(&self, path: &Path) -> Result<LintResult, LinterError> {
        let mut host = self
            .plugin_host
            .lock()
            .map_err(|_| LinterError::Internal("Plugin host mutex poisoned".to_string()))?;

        let enabled_rules_vec = self.config.enabled_rules();
        let enabled_rules: HashSet<&str> = enabled_rules_vec.iter().map(|(n, _)| *n).collect();

        lint_file_internal(
            path,
            &mut host,
            &self.tokenizer,
            &self.config_hash,
            &self.cache,
            &enabled_rules,
            self.config.timings,
        )
    }

    pub fn lint_patterns(&self, patterns: &[String]) -> LintFilesResult {
        let base_dir = self.config.base_dir.as_deref().unwrap_or(Path::new("."));
        let files = self.discover_files(patterns, base_dir)?;
        self.lint_files(&files)
    }

    fn discover_files(
        &self,
        patterns: &[String],
        base_dir: &Path,
    ) -> Result<Vec<PathBuf>, LinterError> {
        let mut files = Vec::new();

        let mut glob_builder = GlobSetBuilder::new();
        let mut has_globs = false;

        for pattern in patterns {
            let path = Path::new(pattern);
            if path.exists() && path.is_file() {
                if let Ok(abs_path) = path.canonicalize() {
                    if self
                        .exclude_globs
                        .as_ref()
                        .is_some_and(|excludes| excludes.is_match(&abs_path))
                    {
                        continue;
                    }

                    if self
                        .include_globs
                        .as_ref()
                        .is_some_and(|includes| !includes.is_match(&abs_path))
                    {
                        continue;
                    }

                    files.push(abs_path);
                }
            } else {
                let glob = Glob::new(pattern).map_err(|e| {
                    LinterError::config(format!("Invalid pattern '{}': {}", pattern, e))
                })?;
                glob_builder.add(glob);
                has_globs = true;
            }
        }

        if has_globs {
            let glob_set = glob_builder
                .build()
                .map_err(|e| LinterError::config(format!("Failed to build globset: {}", e)))?;

            for entry in WalkDir::new(base_dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() && glob_set.is_match(path) {
                    if let Some(ref excludes) = self.exclude_globs
                        && excludes.is_match(path)
                    {
                        continue;
                    }

                    if let Some(ref includes) = self.include_globs
                        && !includes.is_match(path)
                    {
                        continue;
                    }

                    files.push(path.to_path_buf());
                }
            }
        }

        files.sort();
        files.dedup();

        info!("Discovered {} files to lint", files.len());
        Ok(files)
    }

    pub fn lint_files(&self, paths: &[PathBuf]) -> LintFilesResult {
        lint_files_parallel(
            paths,
            &self.config,
            &self.config_hash,
            &self.tokenizer,
            &self.cache,
            &self.dynamic_rules,
        )
    }

    pub fn create_plugin_host(&self) -> Result<PluginHost, LinterError> {
        create_plugin_host(&self.config, &self.dynamic_rules)
    }

    pub fn lint_content(
        &self,
        content: &str,
        path: &Path,
    ) -> Result<Vec<tsuzulint_plugin::Diagnostic>, LinterError> {
        let mut host = self
            .plugin_host
            .lock()
            .map_err(|_| LinterError::Internal("Plugin host lock poisoned".to_string()))?;

        lint_content_internal(content, path, &mut host, &self.tokenizer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn test_config() -> (LinterConfig, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = LinterConfig::new();
        config.cache = crate::config::CacheConfig::Detail(crate::config::CacheConfigDetail {
            enabled: true,
            path: temp_dir.path().to_string_lossy().to_string(),
        });
        (config, temp_dir)
    }

    fn test_config_in(base: &Path) -> LinterConfig {
        let cache_dir = base.join(".cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let mut config = LinterConfig::new();
        config.cache = crate::config::CacheConfig::Detail(crate::config::CacheConfigDetail {
            enabled: true,
            path: cache_dir.to_string_lossy().to_string(),
        });
        config
    }

    #[test]
    fn test_linter_new() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config);
        assert!(linter.is_ok());
    }

    #[test]
    fn test_build_globset() {
        let patterns = vec!["**/*.md".to_string(), "*.txt".to_string()];
        let result = Linter::build_globset(&patterns);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_build_globset_empty() {
        let patterns: Vec<String> = vec![];
        let result = Linter::build_globset(&patterns);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_globset_invalid_pattern() {
        let patterns = vec!["[invalid".to_string()];
        let result = Linter::build_globset(&patterns);
        assert!(result.is_err());
    }

    #[test]
    fn test_linter_with_cache_disabled() {
        let (mut config, _temp) = test_config();
        config.cache = crate::config::CacheConfig::Boolean(false);

        let linter = Linter::new(config).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");
        std::fs::write(&file_path, "content").unwrap();
        let result1 = linter.lint_file(&file_path).unwrap();
        let result2 = linter.lint_file(&file_path).unwrap();
        assert!(
            !result1.from_cache,
            "cache disabled: first lint should not be from cache"
        );
        assert!(
            !result2.from_cache,
            "cache disabled: second lint should not be from cache"
        );
    }

    #[test]
    fn test_linter_with_include_patterns() {
        let (mut config, _temp) = test_config();
        config.include = vec!["**/*.md".to_string()];

        let linter = Linter::new(config).unwrap();
        assert!(linter.include_globs.is_some());
    }

    #[test]
    fn test_linter_with_exclude_patterns() {
        let (mut config, _temp) = test_config();
        config.exclude = vec!["**/node_modules/**".to_string()];

        let linter = Linter::new(config).unwrap();
        assert!(linter.exclude_globs.is_some());
    }

    #[test]
    fn test_build_globset_multiple_patterns() {
        let patterns = vec![
            "**/*.md".to_string(),
            "**/*.txt".to_string(),
            "docs/**/*".to_string(),
        ];
        let result = Linter::build_globset(&patterns);
        assert!(result.is_ok());

        let globset = result.unwrap().unwrap();
        assert!(globset.is_match("file.md"));
        assert!(globset.is_match("dir/file.txt"));
        assert!(globset.is_match("docs/readme.md"));
    }

    #[test]
    fn test_lint_files_parallel_empty() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let paths: Vec<PathBuf> = vec![];
        let result = linter.lint_files(&paths);
        assert!(result.is_ok());

        let (successes, failures) = result.unwrap();
        assert!(successes.is_empty());
        assert!(failures.is_empty());
    }

    #[test]
    fn test_lint_files_parallel_nonexistent_files() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let paths = vec![
            PathBuf::from("/nonexistent/file1.md"),
            PathBuf::from("/nonexistent/file2.txt"),
        ];
        let result = linter.lint_files(&paths);
        assert!(result.is_ok());

        let (successes, failures) = result.unwrap();
        assert!(successes.is_empty());
        assert_eq!(failures.len(), 2);
    }

    #[test]
    fn test_create_plugin_host() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let result = linter.create_plugin_host();
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_configured_rules_static() {
        let (config, _temp) = test_config();
        let mut host = PluginHost::new();

        load_configured_rules(&config, &mut host);
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    fn test_linter_config_hash_caching() {
        let (config, _temp) = test_config();
        let expected_hash = config.hash().unwrap();
        let linter = Linter::new(config).unwrap();

        assert_eq!(linter.config_hash, expected_hash);
    }

    #[test]
    fn test_lint_content_with_simple_rule() {
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test_lint_content_with_simple_rule: WASM build failed");
            return;
        };

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();
        linter
            .load_rule(&wasm_path)
            .expect("Failed to load test rule");

        let content = "This text contains an error keyword.";
        let path = Path::new("test.md");

        let diagnostics = linter.lint_content(content, path).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, "test-rule");
        assert_eq!(diagnostics[0].message, "Found error keyword");
        assert_eq!(diagnostics[0].span.start, 22);
        assert_eq!(diagnostics[0].span.end, 27);

        let clean_content = "This text is clean.";
        let diagnostics = linter.lint_content(clean_content, path).unwrap();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_discover_files_respects_exclude() {
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.md");
        let node_modules = temp_dir.path().join("node_modules");
        fs::create_dir(&node_modules).unwrap();
        let excluded_file = node_modules.join("excluded.md");

        fs::write(&test_file, "# Test").unwrap();
        fs::write(&excluded_file, "# Excluded").unwrap();

        let mut config = test_config_in(temp_dir.path());
        config.exclude = vec!["**/node_modules/**".to_string()];

        let linter = Linter::new(config).unwrap();

        let files = linter
            .discover_files(&["**/*.md".to_string()], temp_dir.path())
            .unwrap();

        assert!(files.iter().any(|f| f.ends_with("test.md")));
        assert!(
            !files
                .iter()
                .any(|f| f.to_string_lossy().contains("node_modules"))
        );
    }

    #[test]
    fn test_discover_files_respects_include() {
        let temp_dir = tempdir().unwrap();
        let md_file = temp_dir.path().join("test.md");
        let txt_file = temp_dir.path().join("test.txt");

        fs::write(&md_file, "# Test").unwrap();
        fs::write(&txt_file, "Test").unwrap();

        let mut config = test_config_in(temp_dir.path());
        config.include = vec!["**/*.md".to_string()];

        let linter = Linter::new(config).unwrap();

        let files = linter
            .discover_files(&["**/*".to_string()], temp_dir.path())
            .unwrap();

        assert!(files.iter().any(|f| f.ends_with("test.md")));
        assert!(!files.iter().any(|f| f.ends_with("test.txt")));
    }

    #[test]
    fn test_discover_files_deduplicates() {
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test").unwrap();

        let config = test_config_in(temp_dir.path());
        let linter = Linter::new(config).unwrap();

        let files = linter
            .discover_files(&["*.md".to_string(), "*.md".to_string()], temp_dir.path())
            .unwrap();

        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_discover_files_multiple_glob_patterns() {
        let temp_dir = tempdir().unwrap();
        let md_file = temp_dir.path().join("test.md");
        let txt_file = temp_dir.path().join("test.txt");
        let rs_file = temp_dir.path().join("test.rs");

        fs::write(&md_file, "# Test").unwrap();
        fs::write(&txt_file, "Test").unwrap();
        fs::write(&rs_file, "fn main() {}").unwrap();

        let config = test_config_in(temp_dir.path());
        let linter = Linter::new(config).unwrap();

        let files = linter
            .discover_files(
                &["**/*.md".to_string(), "**/*.txt".to_string()],
                temp_dir.path(),
            )
            .unwrap();

        assert!(
            files.iter().any(|f| f.ends_with("test.md")),
            "Should find .md file"
        );
        assert!(
            files.iter().any(|f| f.ends_with("test.txt")),
            "Should find .txt file"
        );
        assert!(
            !files.iter().any(|f| f.ends_with("test.rs")),
            "Should not find .rs file"
        );
        assert_eq!(files.len(), 2, "Should find exactly 2 files");
    }

    #[test]
    fn test_discover_files_invalid_glob() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let result = linter.discover_files(&["[invalid-glob".to_string()], Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_linter_with_multiple_patterns() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let patterns = vec!["*.md".to_string(), "*.txt".to_string()];
        let result = linter.lint_patterns(&patterns);

        assert!(result.is_ok());
    }

    #[test]
    fn test_lint_content_with_empty_string() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let result = linter.lint_content("", &PathBuf::from("test.md"));
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_lint_content_with_unknown_extension() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let result = linter.lint_content("Hello", &PathBuf::from("test.xyz"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_lint_file_success() {
        let (mut config, temp_dir) = test_config();

        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule".to_string(),
        ));
        config.options.insert(
            "test-rule".to_string(),
            crate::config::RuleOption::Enabled(true),
        );

        config.timings = true;

        let linter = Linter::new(config).unwrap();

        let mut rule_loaded = false;

        if let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() {
            linter
                .load_rule(&wasm_path)
                .expect("Failed to load test rule");
            rule_loaded = true;
        } else {
            println!("WASM build failed, running test without rules");
        }

        let file_path = temp_dir.path().join("test_lint_file.md");
        let content = "This file contains an error keyword.";
        fs::write(&file_path, content).unwrap();

        let result = linter.lint_file(&file_path);

        assert!(result.is_ok(), "lint_file should return Ok");
        let lint_result = result.unwrap();

        assert_eq!(lint_result.path, file_path);

        if rule_loaded {
            assert_eq!(lint_result.diagnostics.len(), 1, "Should find 1 diagnostic");
            let diag = &lint_result.diagnostics[0];
            assert_eq!(diag.rule_id, "test-rule");
            assert_eq!(diag.message, "Found error keyword");
        } else {
            assert!(
                lint_result.diagnostics.is_empty(),
                "No rules loaded, should be clean"
            );
        }

        let clean_path = temp_dir.path().join("clean.md");
        fs::write(&clean_path, "This file is clean.").unwrap();

        let clean_result = linter.lint_file(&clean_path).unwrap();
        assert!(
            clean_result.diagnostics.is_empty(),
            "Clean file should have no diagnostics"
        );
    }

    #[test]
    fn test_load_rule_absolute_path_security() {
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test: WASM build failed");
            return;
        };

        let temp_dir = tempdir().unwrap();
        let manifest_path = temp_dir.path().join("tsuzulint-rule.json");
        let dest_wasm_path = temp_dir.path().join("rule.wasm");

        fs::copy(&wasm_path, &dest_wasm_path).unwrap();

        let json = r#"{
            "rule": {
                "name": "abs-path-rule",
                "version": "1.0.0",
                "description": "Test rule",
                "fixable": false
            },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;
        fs::write(&manifest_path, json).unwrap();

        let abs_manifest_path = manifest_path.canonicalize().unwrap();
        assert!(abs_manifest_path.is_absolute());

        let mut config = LinterConfig::new();
        config.base_dir = Some(temp_dir.path().to_path_buf());

        config.rules.push(crate::config::RuleDefinition::Detail(
            crate::config::RuleDefinitionDetail {
                github: None,
                url: None,
                path: Some(abs_manifest_path.to_string_lossy().to_string()),
                r#as: None,
            },
        ));

        let linter = Linter::new(config).unwrap();
        let host = linter.plugin_host.lock().unwrap();

        let loaded: Vec<String> = host.loaded_rules().cloned().collect();
        assert!(
            !loaded.contains(&"abs-path-rule".to_string()),
            "Security risk: Absolute path rule was loaded! Loaded rules: {:?}",
            loaded
        );
    }

    #[test]
    fn test_lint_files_partial_failure() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");
        std::fs::write(&file_path, "content").unwrap();

        let paths = vec![file_path, PathBuf::from("/nonexistent/file.md")];

        let result = linter.lint_files(&paths);
        assert!(result.is_ok());

        let (successes, failures) = result.unwrap();
        assert_eq!(successes.len(), 1);
        assert_eq!(failures.len(), 1);

        assert!(failures[0].0.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_lint_caching() {
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test_lint_caching: WASM build failed");
            return;
        };

        let (mut config, temp_dir) = test_config();

        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule".to_string(),
        ));
        config.options.insert(
            "test-rule".to_string(),
            crate::config::RuleOption::Enabled(true),
        );

        config.timings = true;

        let linter = Linter::new(config).unwrap();
        linter.load_rule(&wasm_path).expect("Failed to load rule");

        let file_path = temp_dir.path().join("test.md");
        std::fs::write(&file_path, "Clean content").unwrap();

        let result1 = linter.lint_file(&file_path).unwrap();
        assert!(!result1.from_cache);

        let result2 = linter.lint_file(&file_path).unwrap();
        assert!(result2.from_cache);
    }

    #[test]
    fn test_lint_patterns_expansion() {
        let (mut config, temp_dir) = test_config();
        config.base_dir = Some(temp_dir.path().to_path_buf());

        let linter = Linter::new(config).unwrap();

        let dir = temp_dir.path();
        fs::write(dir.join("a.md"), "").unwrap();
        fs::write(dir.join("b.md"), "").unwrap();
        fs::write(dir.join("c.txt"), "").unwrap();

        let pattern = "*.md".to_string();

        let (successes, _failures) = linter.lint_patterns(&[pattern]).unwrap();
        assert_eq!(successes.len(), 2);
    }

    #[test]
    fn test_lint_file_too_large() {
        let (config, temp_dir) = test_config();
        let linter = Linter::new(config).unwrap();

        let large_file = temp_dir.path().join("large.txt");
        let file = fs::File::create(&large_file).unwrap();
        file.set_len(crate::file_linter::MAX_FILE_SIZE + 1).unwrap();

        let result = linter.lint_file(&large_file);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("File size exceeds limit"));
    }

    #[test]
    #[cfg(unix)]
    fn test_lint_file_rejects_special_files() {
        use std::process::Command;

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let fifo_path = temp_dir.path().join("test.fifo");
        let status = Command::new("mkfifo")
            .arg(&fifo_path)
            .status()
            .expect("mkfifo not available");
        assert!(status.success(), "Failed to create FIFO");

        let result = linter.lint_file(&fifo_path);
        assert!(result.is_err(), "lint_file should reject a FIFO");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Not a regular file"));
    }

    #[test]
    fn test_lint_files_poisoned_dynamic_rules_mutex() {
        let (config, temp_dir) = test_config();
        let linter = Linter::new(config).unwrap();

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = linter.dynamic_rules.lock().unwrap();
            panic!("intentional poison");
        }));

        assert!(linter.dynamic_rules.lock().is_err());

        let file_path = temp_dir.path().join("test_poison.md");
        std::fs::write(&file_path, "Hello").unwrap();

        let result = linter.lint_files(&[file_path]);
        assert!(result.is_ok());

        let (successes, failures) = result.unwrap();
        assert!(successes.is_empty());
        assert_eq!(failures.len(), 1);

        let error_msg = failures[0].1.to_string();
        assert!(
            error_msg.contains("Failed to initialize plugin host"),
            "Expected 'Failed to initialize plugin host' in error, got: {}",
            error_msg
        );
    }

    #[test]
    fn test_lint_content_with_special_characters() {
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test: WASM build failed");
            return;
        };

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();
        linter
            .load_rule(&wasm_path)
            .expect("Failed to load test rule");

        let content = "This contains \"error\".";
        let path = Path::new("special.md");

        let diagnostics = linter.lint_content(content, path).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, "test-rule");
        assert_eq!(diagnostics[0].message, "Found error keyword");
        assert_eq!(diagnostics[0].span.start, 15);
        assert_eq!(diagnostics[0].span.end, 20);
    }

    #[test]
    fn test_lint_file_output_sorted() {
        let (mut config, temp_dir) = test_config();

        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule".to_string(),
        ));
        config.options.insert(
            "test-rule".to_string(),
            crate::config::RuleOption::Enabled(true),
        );

        config.timings = true;

        let linter = Linter::new(config).unwrap();

        if let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() {
            linter
                .load_rule(&wasm_path)
                .expect("Failed to load test rule");
        } else {
            println!("WASM build failed, skipping sort test");
            return;
        }

        let file_path = temp_dir.path().join("test_sort.md");
        let content = "0123456789error0123456789error0123456789error";
        fs::write(&file_path, content).unwrap();

        let result = linter.lint_file(&file_path).unwrap();

        let diags = result.diagnostics;
        assert_eq!(diags.len(), 3);

        assert!(diags[0].span.start < diags[1].span.start);
        assert!(diags[1].span.start < diags[2].span.start);
    }

    #[test]
    fn test_lint_file_multiple_global_rules() {
        let (mut config, temp_dir) = test_config();

        // Register two global rules
        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule".to_string(),
        ));
        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule-2".to_string(),
        ));
        config.options.insert(
            "test-rule".to_string(),
            crate::config::RuleOption::Enabled(true),
        );
        config.options.insert(
            "test-rule-2".to_string(),
            crate::config::RuleOption::Enabled(true),
        );

        config.timings = true;

        let linter = Linter::new(config).unwrap();

        if let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() {
            // Load rule initially (registers as "test-rule" based on internal manifest)
            linter
                .load_rule(&wasm_path)
                .expect("Failed to load test rule");

            // Rename it to "test-rule-2" to free up "test-rule" slot
            {
                let mut host = linter.plugin_host.lock().unwrap();
                host.rename_rule("test-rule", "test-rule-2", None).unwrap();
            }

            // Load it again to populate "test-rule"
            linter
                .load_rule(&wasm_path)
                .expect("Failed to load test rule 2");
        } else {
            println!("WASM build failed, skipping test");
            return;
        }

        let file_path = temp_dir.path().join("test_multi.md");
        let content = "error";
        fs::write(&file_path, content).unwrap();

        let result = linter.lint_file(&file_path).unwrap();

        // Both rules should have been executed and logged in timings
        // Each rule produces a diagnostic with its own rule_id, so no deduplication
        assert!(
            result.timings.contains_key("test-rule"),
            "Missing timing for test-rule"
        );
        assert!(
            result.timings.contains_key("test-rule-2"),
            "Missing timing for test-rule-2"
        );
        assert_eq!(
            result.diagnostics.len(),
            2,
            "Each rule should produce one diagnostic"
        );
    }

    #[test]
    fn test_lint_file_block_rule() {
        use crate::Linter;
        use crate::config::{LinterConfig, RuleDefinition, RuleOption};
        use tsuzulint_plugin::{IsolationLevel, RuleManifest};

        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = LinterConfig::new();
        config.cache = crate::config::CacheConfig::Detail(crate::config::CacheConfigDetail {
            enabled: true,
            path: temp_dir.path().to_string_lossy().to_string(),
        });

        // Register a block rule
        config
            .rules
            .push(RuleDefinition::Simple("block-rule".to_string()));
        config
            .options
            .insert("block-rule".to_string(), RuleOption::Enabled(true));

        config.timings = true;

        let linter = Linter::new(config).unwrap();

        if let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() {
            // Load rule initially
            linter
                .load_rule(&wasm_path)
                .expect("Failed to load test rule");

            // Change it to be a block rule
            {
                let mut host = linter.plugin_host.lock().unwrap();
                let manifest = RuleManifest::new("block-rule", "1.0.0")
                    .with_isolation_level(IsolationLevel::Block);

                // Rename "test-rule" to "block-rule" and update manifest
                host.rename_rule("test-rule", "block-rule", Some(manifest))
                    .unwrap();
            }
        } else {
            println!("WASM build failed, skipping test");
            return;
        }

        let file_path = temp_dir.path().join("test_block.md");
        // Ensure there are some blocks (paragraphs)
        let content = "Block 1.\n\nBlock 2 with error.";
        fs::write(&file_path, content).unwrap();

        let result = linter.lint_file(&file_path).unwrap();

        // The rule checks for "error". It should be found in the second block.
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].rule_id, "block-rule");

        // Check timings to verify it ran as a block rule
        assert!(result.timings.contains_key("block-rule"));
    }
}
