//! Core linter engine.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use tsuzulint_ast::{AstArena, NodeType, TxtNode};
use tsuzulint_cache::{CacheEntry, CacheManager, entry::BlockCacheEntry};
use tsuzulint_parser::{MarkdownParser, Parser, PlainTextParser};
use tsuzulint_plugin::{IsolationLevel, PluginHost};

use crate::resolver::PluginResolver;
use crate::{LintResult, LinterConfig, LinterError};

/// Result type for lint_files and lint_patterns methods.
///
/// Contains a tuple of:
/// - Successful lint results
/// - Failed files with their errors (path and error)
pub type LintFilesResult = Result<(Vec<LintResult>, Vec<(PathBuf, LinterError)>), LinterError>;

/// The core linter engine.
///
/// Orchestrates file discovery, parsing, rule execution, and caching.
pub struct Linter {
    /// Linter configuration.
    config: LinterConfig,
    /// Pre-computed hash of the configuration.
    config_hash: String,
    /// Plugin host for WASM rules.
    plugin_host: Mutex<PluginHost>,
    /// Cache manager.
    cache: Mutex<CacheManager>,
    /// Rules added via load_rule at runtime.
    dynamic_rules: Mutex<Vec<PathBuf>>,
    /// Include glob patterns.
    include_globs: Option<GlobSet>,
    /// Exclude glob patterns.
    exclude_globs: Option<GlobSet>,
}

impl Linter {
    /// Creates a new linter with the given configuration.
    pub fn new(config: LinterConfig) -> Result<Self, LinterError> {
        let cache_dir = PathBuf::from(&config.cache_dir);
        let mut cache = CacheManager::new(cache_dir);

        if !config.cache {
            cache.disable();
        }

        // Load cache from disk
        if let Err(e) = cache.load() {
            warn!("Failed to load cache: {}", e);
        }

        // Build glob patterns
        let include_globs = Self::build_globset(&config.include)?;
        let exclude_globs = Self::build_globset(&config.exclude)?;

        // Initialize plugin host
        let mut host = PluginHost::new();

        // Load configured plugins and rules
        Self::load_configured_rules(&config, &mut host);

        // Pre-compute config hash
        let config_hash = config.hash();

        Ok(Self {
            config,
            config_hash,
            plugin_host: Mutex::new(host),
            cache: Mutex::new(cache),
            dynamic_rules: Mutex::new(Vec::new()),
            include_globs,
            exclude_globs,
        })
    }

    /// Builds a GlobSet from patterns.
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

    /// Loads a WASM rule.
    ///
    /// This rule will also be loaded for parallel processing in `lint_files()`.
    pub fn load_rule(&self, path: impl AsRef<Path>) -> Result<(), LinterError> {
        let path_buf = path.as_ref().to_path_buf();

        // Load rule into the shared PluginHost (scope to release lock before acquiring dynamic_rules lock)
        {
            let mut host = self
                .plugin_host
                .lock()
                .map_err(|_| LinterError::Internal("Plugin host mutex poisoned".to_string()))?;
            host.load_rule(&path_buf)?;
        }

        // Record dynamic rule for parallel processing (after releasing plugin_host lock)
        {
            let mut list = self
                .dynamic_rules
                .lock()
                .map_err(|_| LinterError::Internal("Dynamic rules mutex poisoned".to_string()))?;
            list.push(path_buf);
        }
        Ok(())
    }

    /// Lints files matching the given patterns.
    ///
    /// Returns a tuple of (successful results, failed files with errors).
    pub fn lint_patterns(&self, patterns: &[String]) -> LintFilesResult {
        let files = self.discover_files(patterns)?;
        self.lint_files(&files)
    }

    /// Discovers files matching the given patterns.
    fn discover_files(&self, patterns: &[String]) -> Result<Vec<PathBuf>, LinterError> {
        let mut files = Vec::new();

        for pattern in patterns {
            let glob = Glob::new(pattern).map_err(|e| {
                LinterError::config(format!("Invalid pattern '{}': {}", pattern, e))
            })?;
            let matcher = glob.compile_matcher();

            for entry in WalkDir::new(".").into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() && matcher.is_match(path) {
                    // Check exclude patterns
                    if let Some(ref excludes) = self.exclude_globs
                        && excludes.is_match(path)
                    {
                        continue;
                    }

                    // Check include patterns (if specified)
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

    /// Lints a list of files in parallel using rayon.
    ///
    /// Creates a new `PluginHost` instance for each file to ensure thread safety.
    /// While this incurs some overhead from plugin reloading, it avoids lock
    /// contention and allows full utilization of multi-core processors.
    ///
    /// Returns a tuple of (successful results, failed files with errors).
    pub fn lint_files(&self, paths: &[PathBuf]) -> LintFilesResult {
        // Parallel processing using rayon
        // Each file gets its own PluginHost to avoid shared mutable state
        // Collect both successes and failures
        let results: Vec<Result<LintResult, (PathBuf, LinterError)>> = paths
            .par_iter()
            .map(|path| {
                // Create PluginHost for this file
                let mut file_host = self.create_plugin_host().map_err(|e| (path.clone(), e))?;

                self.lint_file_with_host(path, &mut file_host)
                    .map_err(|e| (path.clone(), e))
            })
            .collect();

        // Separate successes and failures
        let mut successes = Vec::new();
        let mut failures = Vec::new();
        for result in results {
            match result {
                Ok(lint_result) => successes.push(lint_result),
                Err((path, error)) => {
                    warn!("Failed to lint {}: {}", path.display(), error);
                    failures.push((path, error));
                }
            }
        }

        // Save cache (handle mutex poison error gracefully)
        match self.cache.lock() {
            Ok(cache) => {
                if let Err(e) = cache.save() {
                    warn!("Failed to save cache: {}", e);
                }
            }
            Err(poison) => {
                warn!("Cache mutex poisoned, attempting recovery: {}", poison);
                // Still try to save via the poisoned guard
                if let Err(e) = poison.into_inner().save() {
                    warn!("Failed to save cache after recovery: {}", e);
                }
            }
        }

        Ok((successes, failures))
    }

    /// Creates a new PluginHost with the same configuration as the linter.
    ///
    /// Used for parallel processing where each thread needs its own PluginHost.
    fn create_plugin_host(&self) -> Result<PluginHost, LinterError> {
        let mut host = PluginHost::new();

        // Load configured plugins and rules
        Self::load_configured_rules(&self.config, &mut host);

        // Load dynamically added rules (via load_rule API)
        {
            let dynamic = self
                .dynamic_rules
                .lock()
                .map_err(|_| LinterError::Internal("Dynamic rules mutex poisoned".to_string()))?;
            for path in dynamic.iter() {
                debug!("Loading dynamic plugin from {}", path.display());
                if let Err(e) = host.load_rule(path) {
                    warn!("Failed to load dynamic plugin '{}': {}", path.display(), e);
                }
            }
        }

        Ok(host)
    }

    /// Loads plugins and rules into the given PluginHost based on config.
    ///
    /// This is a shared helper used by both `new()` and `create_plugin_host()`.
    fn load_configured_rules(config: &LinterConfig, host: &mut PluginHost) {
        // Helper to load a rule/plugin by name/path
        let load_plugin = |name: &str, host: &mut PluginHost| match PluginResolver::resolve(
            name,
            config.base_dir.as_deref(),
        ) {
            Some(path) => {
                debug!("Loading plugin '{}' from {}", name, path.display());
                if let Err(e) = host.load_rule(&path) {
                    warn!("Failed to load plugin '{}': {}", name, e);
                }
            }
            None => {
                debug!(
                    "Plugin '{}' not found. Checked .tsuzulint/plugins/ and global directories.",
                    name
                );
            }
        };

        // Load legacy plugins list
        for plugin_name in &config.plugins {
            load_plugin(plugin_name, host);
        }

        // Load rules from new rules array
        for rule_def in &config.rules {
            use crate::config::RuleDefinition;
            match rule_def {
                RuleDefinition::Simple(name) => {
                    load_plugin(name, host);
                }
                RuleDefinition::Detail(detail) => {
                    // Prioritize path, then github/url (not fully implemented yet)
                    if let Some(path) = &detail.path {
                        // detail.path points to tsuzulint-rule.json manifest
                        let manifest_path = if let Some(base) = &config.base_dir {
                            base.join(path)
                        } else {
                            PathBuf::from(path)
                        };

                        match crate::rule_manifest::load_rule_manifest(&manifest_path) {
                            Ok((manifest, wasm_path)) => {
                                let rule_name = detail
                                    .r#as
                                    .clone()
                                    .unwrap_or_else(|| manifest.rule.name.clone());
                                debug!(
                                    "Loading rule '{}' from manifest: {}",
                                    rule_name,
                                    manifest_path.display()
                                );
                                match host.load_rule(&wasm_path) {
                                    Ok(loaded_manifest) => {
                                        // The rule is loaded with the name defined in the WASM binary.
                                        // We want to use the name from tsuzulint-rule.json (or the alias).
                                        // Also we want to associate the manifest from JSON.
                                        let internal_name = loaded_manifest.name.clone();
                                        // Convert ExternalRuleManifest to tsuzulint_plugin::RuleManifest
                                        let plugin_manifest = convert_manifest(&manifest);

                                        if let Err(e) = host.rename_rule(
                                            &internal_name,
                                            &rule_name,
                                            Some(plugin_manifest),
                                        ) {
                                            warn!(
                                                "Failed to register rule '{}' (loaded as '{}'): {}",
                                                rule_name, internal_name, e
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to load rule '{}': {}", rule_name, e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to load rule manifest '{}': {}",
                                    manifest_path.display(),
                                    e
                                );
                            }
                        }
                    } else if let Some(github) = &detail.github {
                        // Placeholder for github fetching
                        warn!("GitHub rule fetching not yet implemented: {}", github);
                    } else if let Some(url) = &detail.url {
                        warn!("URL rule fetching not yet implemented: {}", url);
                    }
                }
            }
        }
    }

    /// Selects an appropriate parser for the file extension.
    fn select_parser(&self, extension: &str) -> Box<dyn Parser> {
        let md_parser = MarkdownParser::new();
        let txt_parser = PlainTextParser::new();

        if md_parser.can_parse(extension) {
            Box::new(md_parser)
        } else if txt_parser.can_parse(extension) {
            Box::new(txt_parser)
        } else {
            // Default to plain text
            Box::new(txt_parser)
        }
    }

    /// Lints a single file with a provided PluginHost.
    ///
    /// Used for parallel processing where each thread has its own PluginHost.
    fn lint_file_with_host(
        &self,
        path: &Path,
        host: &mut PluginHost,
    ) -> Result<LintResult, LinterError> {
        self.lint_file_internal(path, host)
    }

    /// Internal implementation for linting a single file.
    fn lint_file_internal(
        &self,
        path: &Path,
        host: &mut PluginHost,
    ) -> Result<LintResult, LinterError> {
        debug!("Linting {}", path.display());

        // Read file content
        let content = fs::read_to_string(path)
            .map_err(|e| LinterError::file(format!("Failed to read {}: {}", path.display(), e)))?;

        let content_hash = CacheManager::hash_content(&content);
        let config_hash = self.config_hash.clone();
        let rule_versions = Self::get_rule_versions_from_host(host);

        // 1. Check full cache first
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
            if cache.is_valid(path, &content_hash, &config_hash, &rule_versions)
                && let Some(entry) = cache.get(path)
            {
                debug!("Using cached result for {}", path.display());
                return Ok(LintResult::cached(
                    path.to_path_buf(),
                    entry.diagnostics.clone(),
                ));
            }
        }

        // Find appropriate parser
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let parser = self.select_parser(extension);

        // Parse the file
        let arena = AstArena::new();
        let ast = parser
            .parse(&arena, &content)
            .map_err(|e| LinterError::parse(e.to_string()))?;

        // Extract blocks for incremental analysis
        let current_blocks = self.extract_blocks(&ast, &content);

        // 2. Incremental Caching Strategy
        // If file changed, try to reuse diagnostics for unchanged blocks
        let (reused_diagnostics, matched_mask) = {
            let cache = self
                .cache
                .lock()
                .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
            cache.reconcile_blocks(path, &current_blocks, &config_hash, &rule_versions)
        };

        // Prepare diagnostics collection
        // We track global diagnostics separately to avoid polluting block cache later
        let mut global_diagnostics = Vec::new();
        let mut block_diagnostics = Vec::new();
        let mut timings = HashMap::new();

        // Run rules
        {
            // A. Run Global Rules
            // Global rules must always run on the full document if anything changed
            // because they depend on the full context.
            let global_rule_names = self.get_rule_names_by_isolation(host, IsolationLevel::Global);
            if !global_rule_names.is_empty() {
                let ast_json = self.ast_to_json(&ast, &content);
                for rule in global_rule_names {
                    let start = Instant::now();
                    match host.run_rule(&rule, &ast_json, &content, path.to_str()) {
                        Ok(diags) => global_diagnostics.extend(diags),
                        Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                    }
                    if self.config.timings {
                        timings.insert(rule, start.elapsed());
                    }
                }
            }

            // B. Run Block Rules on CHANGED/NEW blocks
            let block_rule_names = self.get_rule_names_by_isolation(host, IsolationLevel::Block);
            if !block_rule_names.is_empty() {
                // Collect AST nodes for changed blocks
                // We map `matched_mask` back to actual AST nodes by traversing.
                let mut block_index = 0;
                self.visit_blocks(&ast, &mut |node| {
                    if block_index < matched_mask.len() {
                        if !matched_mask[block_index] {
                            // This block changed. Run block rules on it.
                            let node_json = self.ast_to_json(node, &content);
                            for rule in &block_rule_names {
                                let start = Instant::now();
                                match host.run_rule(rule, &node_json, &content, path.to_str()) {
                                    Ok(diags) => block_diagnostics.extend(diags),
                                    Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                                }
                                if self.config.timings {
                                    *timings.entry(rule.clone()).or_insert(Duration::new(0, 0)) +=
                                        start.elapsed();
                                }
                            }
                        }
                        block_index += 1;
                    }
                });
            }
        }

        // Deduplicate diagnostics
        // We combine reused (unchanged blocks), global (fresh), and block (changed blocks) diagnostics.
        let mut all_diagnostics = reused_diagnostics;
        all_diagnostics.extend(global_diagnostics.iter().cloned());
        all_diagnostics.extend(block_diagnostics);

        let mut final_diagnostics = Vec::new();
        let mut seen_diagnostics = HashSet::new();

        // Also track which diagnostics are "global" so we don't stick them into block cache
        let mut global_keys = HashSet::new();
        for d in &global_diagnostics {
            global_keys.insert((
                d.span.start,
                d.span.end,
                d.message.clone(),
                d.rule_id.clone(),
            ));
        }

        for diag in all_diagnostics {
            let key = (
                diag.span.start,
                diag.span.end,
                diag.message.clone(),
                diag.rule_id.clone(),
            );
            if seen_diagnostics.insert(key) {
                final_diagnostics.push(diag);
            }
        }

        // Update cache
        // We need to associate diagnostics with blocks for NEXT time.
        // We ensure we ONLY store diagnostics that belong to the block and are NOT global.

        let new_blocks: Vec<BlockCacheEntry> = current_blocks
            .into_iter()
            .map(|mut block| {
                // Filter diagnostics that fall within this block's span
                let block_diags: Vec<_> = final_diagnostics
                    .iter()
                    .filter(|d| {
                        // Check strict inclusion
                        let in_block =
                            d.span.start >= block.span.start && d.span.end <= block.span.end;
                        if !in_block {
                            return false;
                        }

                        // Check if it's a global diagnostic
                        let key = (
                            d.span.start,
                            d.span.end,
                            d.message.clone(),
                            d.rule_id.clone(),
                        );
                        !global_keys.contains(&key)
                    })
                    .cloned()
                    .collect();
                block.diagnostics = block_diags;
                block
            })
            .collect();

        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
            let entry = CacheEntry::new(
                content_hash,
                config_hash,
                rule_versions,
                final_diagnostics.clone(),
                new_blocks,
            );
            cache.set(path.to_path_buf(), entry);
        }

        let mut result = LintResult::new(path.to_path_buf(), final_diagnostics);
        result.timings = timings;
        Ok(result)
    }

    /// Extracts blocks from AST for caching.
    fn extract_blocks(&self, ast: &TxtNode, content: &str) -> Vec<BlockCacheEntry> {
        let mut blocks = Vec::new();

        // Helper to traverse and collect blocks
        self.visit_blocks(ast, &mut |node| {
            // Compute hash for this block
            // Use byte-safe operations to prevent panics
            let start = node.span.start as usize;
            let end = node.span.end as usize;
            let content_bytes = content.as_bytes();

            // Safety check for bounds
            if start <= content_bytes.len() && end <= content_bytes.len() && start <= end {
                let bytes = &content_bytes[start..end];
                let block_content = String::from_utf8_lossy(bytes);
                let hash = CacheManager::hash_content(&block_content);

                blocks.push(BlockCacheEntry {
                    hash,
                    span: node.span,
                    diagnostics: Vec::new(), // Will be populated later
                });
            } else {
                warn!(
                    "Block span {:?} out of bounds for content length {}",
                    node.span,
                    content.len()
                );
            }
        });

        blocks
    }

    /// Visits nodes that are considered "blocks" (e.g. Paragraphs, Headers).
    /// This defines the granularity of incremental caching.
    fn visit_blocks<F>(&self, node: &TxtNode, f: &mut F)
    where
        F: FnMut(&TxtNode),
    {
        // Define what constitutes a "block".
        // For Markdown, usually top-level children of Document (Paragraph, Heading, List, etc.)
        // For now, we assume direct children of Root are blocks.
        if node.node_type == NodeType::Document {
            for child in node.children.iter() {
                f(child);
            }
        }
    }

    /// Gets rule names filtered by isolation level.
    fn get_rule_names_by_isolation(
        &self,
        host: &PluginHost,
        level: tsuzulint_plugin::IsolationLevel,
    ) -> Vec<String> {
        let mut names = Vec::new();
        // Only run rules that are enabled in options
        let enabled_rules = self.config.enabled_rules();
        let enabled_names: HashSet<&str> = enabled_rules.iter().map(|(n, _)| *n).collect();

        for name in host.loaded_rules() {
            // Check if rule is enabled
            if enabled_names.contains(name.as_str())
                && let Some(manifest) = host.get_manifest(name)
                && manifest.isolation_level == level
            {
                names.push(name.clone());
            }
        }
        names
    }

    /// Lints content directly (for LSP or modify-on-save scenarios).
    pub fn lint_content(
        &self,
        content: &str,
        path: &Path,
    ) -> Result<Vec<tsuzulint_plugin::Diagnostic>, LinterError> {
        // Find appropriate parser
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let parser = self.select_parser(extension);

        // Parse the file
        let arena = AstArena::new();
        let ast = parser
            .parse(&arena, content)
            .map_err(|e| LinterError::parse(e.to_string()))?;

        // Convert AST to JSON for plugin system
        let ast_json = self.ast_to_json(&ast, content);

        // Run rules
        let diagnostics = {
            let mut host = self
                .plugin_host
                .lock()
                .map_err(|_| LinterError::Internal("Plugin host lock poisoned".to_string()))?;
            host.run_all_rules(&ast_json, content, path.to_str())?
        };

        Ok(diagnostics)
    }

    /// Gets the versions of all loaded rules from a PluginHost.
    fn get_rule_versions_from_host(host: &PluginHost) -> HashMap<String, String> {
        let mut versions = HashMap::new();

        for name in host.loaded_rules() {
            if let Some(manifest) = host.get_manifest(name) {
                versions.insert(name.to_string(), manifest.version.clone());
            }
        }

        versions
    }

    /// Converts a TxtNode to JSON for the plugin system.
    fn ast_to_json(&self, node: &tsuzulint_ast::TxtNode, _source: &str) -> serde_json::Value {
        // Simplified JSON representation
        serde_json::json!({
            "type": format!("{}", node.node_type),
            "range": [node.span.start, node.span.end],
            "children": node.children.iter()
                .map(|c| self.ast_to_json(c, _source))
                .collect::<Vec<_>>(),
        })
    }
}

fn convert_manifest(
    external: &tsuzulint_manifest::ExternalRuleManifest,
) -> tsuzulint_plugin::RuleManifest {
    use tsuzulint_manifest::IsolationLevel as ExternalIsolationLevel;
    use tsuzulint_plugin::IsolationLevel as PluginIsolationLevel;

    let isolation_level = match external.rule.isolation_level {
        ExternalIsolationLevel::Global => PluginIsolationLevel::Global,
        ExternalIsolationLevel::Block => PluginIsolationLevel::Block,
    };

    tsuzulint_plugin::RuleManifest {
        name: external.rule.name.clone(),
        version: external.rule.version.clone(),
        description: external.rule.description.clone(),
        fixable: external.rule.fixable,
        node_types: external.rule.node_types.clone(),
        isolation_level,
        schema: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> (LinterConfig, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = LinterConfig::new();
        config.cache_dir = temp_dir.path().to_string_lossy().to_string();
        (config, temp_dir)
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
        config.cache = false;

        let linter = Linter::new(config).unwrap();
        // Verify linter was created successfully with cache disabled
        assert!(linter.include_globs.is_none());
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
    fn test_linter_select_parser_markdown() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let parser = linter.select_parser("md");
        assert_eq!(parser.name(), "markdown");

        let parser = linter.select_parser("markdown");
        assert_eq!(parser.name(), "markdown");
    }

    #[test]
    fn test_linter_select_parser_text() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let parser = linter.select_parser("txt");
        assert_eq!(parser.name(), "text");

        let parser = linter.select_parser("text");
        assert_eq!(parser.name(), "text");
    }

    #[test]
    fn test_linter_select_parser_unknown_defaults_to_text() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let parser = linter.select_parser("unknown");
        assert_eq!(parser.name(), "text");
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
    fn test_linter_ast_to_json() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let arena = AstArena::new();
        let text_node = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let children = arena.alloc_slice_copy(&[*text_node]);
        let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 5), children);

        let json = linter.ast_to_json(&doc, "hello");

        assert_eq!(json["type"], "Document");
        assert!(json["range"].is_array());
        assert!(json["children"].is_array());
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
        // All files should fail since they don't exist
        assert!(successes.is_empty());
        assert_eq!(failures.len(), 2);
    }

    #[test]
    fn test_create_plugin_host() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        // create_plugin_host should succeed even with no plugins configured
        let result = linter.create_plugin_host();
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_configured_rules_static() {
        // Test that load_configured_rules can be called as a static method
        let (config, _temp) = test_config();
        let mut host = PluginHost::new();

        // This should not panic even with empty config
        Linter::load_configured_rules(&config, &mut host);
        // With no plugins configured, no rules should be loaded
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    #[ignore]
    fn test_load_configured_rules_from_manifest() {
        // Setup temporary directory with rule manifest and wasm
        let (_config, dir) = test_config();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        // Copy existing WASM file for testing
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // manifest_dir is crates/tsuzulint_core
        // We look for built rules in the workspace target directory
        // Assuming typical workspace layout: workspace_root/target/...
        // But here we found them in workspace_root/rules/target via find_by_name tool earlier.
        // Let's rely on the relative path from tsuzulint_core: ../../rules/target/...
        let wasm_source = manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("rules/target/wasm32-wasip1/release/texide_rule_no_todo.wasm");

        if wasm_source.exists() {
            fs::copy(&wasm_source, &wasm_path).unwrap();
        } else {
            // Fallback: search in common target directories
            // This is to make test robust across different build environments
            let alt_source = manifest_dir
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target/wasm32-wasip1/release/texide_rule_no_todo.wasm");

            if alt_source.exists() {
                fs::copy(&alt_source, &wasm_path).unwrap();
            } else {
                eprintln!(
                    "SKIPPING TEST: WASM file not found at {} or {}. Run `cargo build --release -p rules` (or similar) to generate it.",
                    wasm_source.display(),
                    alt_source.display()
                );
                return;
            }
        }

        // Create manifest
        let json = r#"{
            "rule": {
                "name": "manifest-test-rule",
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

        // Configure linter
        let mut config = LinterConfig::new();
        config.base_dir = Some(dir.path().to_path_buf());

        // Use relative path from base_dir
        config.rules.push(crate::config::RuleDefinition::Detail(
            crate::config::RuleDefinitionDetail {
                github: None,
                url: None,
                path: Some("tsuzulint-rule.json".to_string()),
                r#as: None, // Should pick up name from manifest
            },
        ));

        // Create linter (this triggers load_configured_rules)
        let linter = Linter::new(config).unwrap();

        // Verify rule is loaded
        let host = linter.plugin_host.lock().unwrap();
        let loaded: Vec<String> = host.loaded_rules().cloned().collect();
        assert!(loaded.contains(&"manifest-test-rule".to_string()));
    }

    #[test]
    fn test_linter_config_hash_caching() {
        let (config, _temp) = test_config();
        let expected_hash = config.hash();
        let linter = Linter::new(config).unwrap();

        // Verify that the hash stored in Linter matches the one computed from config
        assert_eq!(linter.config_hash, expected_hash);
    }

    #[test]
    fn test_lint_content_with_simple_rule() {
        // Build the test rule WASM
        let wasm_path = crate::test_utils::build_simple_rule_wasm();

        let (config, _temp) = test_config();

        // Create linter
        let linter = Linter::new(config).unwrap();

        // Load the rule dynamically
        linter.load_rule(&wasm_path).expect("Failed to load test rule");

        // Test case 1: Content with error
        let content = "This text contains an error keyword.";
        let path = Path::new("test.md");

        let diagnostics = linter.lint_content(content, path).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, "test-rule");
        assert_eq!(diagnostics[0].message, "Found error keyword");
        assert_eq!(diagnostics[0].span.start, 22);
        assert_eq!(diagnostics[0].span.end, 27);

        // Test case 2: Content without error
        let clean_content = "This text is clean.";
        let diagnostics = linter.lint_content(clean_content, path).unwrap();
        assert_eq!(diagnostics.len(), 0);
    }
}
