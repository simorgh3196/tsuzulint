//! Core linter engine.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tsuzulint_text::{SentenceSplitter, Tokenizer};

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

/// Maximum file size to lint (10 MB).
/// Files larger than this will be skipped to prevent DoS via memory exhaustion.
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

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
    /// Tokenizer for text analysis.
    tokenizer: Arc<Tokenizer>,
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

        // Initialize tokenizer
        let tokenizer = Arc::new(Tokenizer::new().map_err(|e| {
            LinterError::Internal(format!("Failed to initialize tokenizer: {}", e))
        })?);

        // Build glob patterns
        let include_globs = Self::build_globset(&config.include)?;
        let exclude_globs = Self::build_globset(&config.exclude)?;

        // Initialize plugin host
        let mut host = PluginHost::new();

        // Load configured plugins and rules
        Self::load_configured_rules(&config, &mut host);

        // Pre-compute config hash
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

    /// Lints a single file using the shared PluginHost.
    ///
    /// For sequential processing or when using the linter's shared PluginHost.
    #[allow(dead_code)]
    fn lint_file(&self, path: &Path) -> Result<LintResult, LinterError> {
        let mut host = self
            .plugin_host
            .lock()
            .map_err(|_| LinterError::Internal("Plugin host mutex poisoned".to_string()))?;
        self.lint_file_internal(path, &mut host)
    }

    /// Lints files matching the given patterns.
    ///
    /// Returns a tuple of (successful results, failed files with errors).
    pub fn lint_patterns(&self, patterns: &[String]) -> LintFilesResult {
        let base_dir = self.config.base_dir.as_deref().unwrap_or(Path::new("."));
        let files = self.discover_files(patterns, base_dir)?;
        self.lint_files(&files)
    }

    /// Discovers files matching the given patterns.
    ///
    /// Walks the `base_dir` directory tree and returns files matching the patterns.
    fn discover_files(
        &self,
        patterns: &[String],
        base_dir: &Path,
    ) -> Result<Vec<PathBuf>, LinterError> {
        let mut files = Vec::new();

        for pattern in patterns {
            // Check if the pattern is a direct file path first
            let path = Path::new(pattern);
            if path.exists() && path.is_file() {
                // Canonicalize to get absolute path for consistent checking
                if let Ok(abs_path) = path.canonicalize() {
                    // Check exclude patterns
                    if self
                        .exclude_globs
                        .as_ref()
                        .is_some_and(|excludes| excludes.is_match(&abs_path))
                    {
                        continue;
                    }

                    // Check include patterns (if specified)
                    // For direct file arguments, we might want to be more lenient,
                    // but for strictness we check includes too (unless includes are empty/not set)
                    if self
                        .include_globs
                        .as_ref()
                        .is_some_and(|includes| !includes.is_match(&abs_path))
                    {
                        continue;
                    }

                    files.push(abs_path);
                    continue;
                }
            }

            let glob = Glob::new(pattern).map_err(|e| {
                LinterError::config(format!("Invalid pattern '{}': {}", pattern, e))
            })?;
            let matcher = glob.compile_matcher();

            for entry in WalkDir::new(base_dir).into_iter().filter_map(|e| e.ok()) {
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

    /// Resolves the manifest path, ensuring security constraints.
    fn resolve_manifest_path(base_dir: Option<&Path>, path: &str) -> Option<PathBuf> {
        let p = Path::new(path);
        // Security check: Prevent absolute paths
        if p.is_absolute() || p.has_root() {
            warn!("Ignoring absolute rule path: {}", path);
            return None;
        }

        // Security check: Prevent directory traversal via ".."
        if p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            warn!("Ignoring rule path containing '..': {}", path);
            return None;
        }

        if let Some(base) = base_dir {
            let joined = base.join(path);
            // Security check: Ensure path resolves within base directory
            // This prevents symlink traversal attacks
            match (joined.canonicalize(), base.canonicalize()) {
                (Ok(canon_path), Ok(canon_base)) => {
                    if canon_path.starts_with(&canon_base) {
                        Some(canon_path)
                    } else {
                        warn!(
                            "Ignoring rule path that resolves outside base directory: {}",
                            path
                        );
                        None
                    }
                }
                (Err(e), _) => {
                    // File might not exist, or permission error.
                    // If we can't canonicalize, we can't verify security fully, but we also can't load it.
                    // So returning the joined path and letting loader fail is one option,
                    // but returning None is safer if we want to enforce existence/security here.
                    // Given this is a security check, we should probably fail safe.
                    // However, standard loader behavior expects "File not found" error.
                    // Let's log warning and return None to match "Ignoring..." behavior.
                    debug!(
                        "Failed to canonicalize rule path '{}': {}",
                        joined.display(),
                        e
                    );
                    None
                }
                (_, Err(e)) => {
                    warn!("Failed to canonicalize base directory: {}", e);
                    None
                }
            }
        } else {
            Some(PathBuf::from(path))
        }
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
                        if let Some(manifest_path) =
                            Self::resolve_manifest_path(config.base_dir.as_deref(), path)
                        {
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

    /// Serializes a value to a RawValue, mapping errors to LinterError.
    fn to_raw_value<T: serde::Serialize>(
        value: &T,
        label: &str,
    ) -> Result<Box<serde_json::value::RawValue>, LinterError> {
        serde_json::value::to_raw_value(value)
            .map_err(|e| LinterError::Internal(format!("Failed to serialize {}: {}", label, e)))
    }

    /// Internal implementation for linting a single file.
    fn lint_file_internal(
        &self,
        path: &Path,
        host: &mut PluginHost,
    ) -> Result<LintResult, LinterError> {
        debug!("Linting {}", path.display());

        // Check file size limit to prevent DoS
        let metadata = fs::metadata(path).map_err(|e| {
            LinterError::file(format!(
                "Failed to read metadata for {}: {}",
                path.display(),
                e
            ))
        })?;

        if metadata.len() > MAX_FILE_SIZE {
            return Err(LinterError::file(format!(
                "File size exceeds limit of {} bytes: {}",
                MAX_FILE_SIZE,
                path.display()
            )));
        }

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

        // Extract ignore ranges (CodeBlock and Code)
        let ignore_ranges = self.extract_ignore_ranges(&ast);

        // Tokenize content
        let tokens = self
            .tokenizer
            .tokenize(&content)
            .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?;
        let sentences = SentenceSplitter::split(&content, &ignore_ranges);

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
                if global_rule_names.len() == 1 {
                    // Optimized path for single rule: avoid RawValue
                    let rule = &global_rule_names[0];
                    let start = Instant::now();
                    match host.run_rule_with_parts(
                        rule,
                        &ast,
                        &content,
                        &tokens,
                        &sentences,
                        path.to_str(),
                    ) {
                        Ok(diags) => global_diagnostics.extend(diags),
                        Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                    }
                    if self.config.timings {
                        timings.insert(rule.clone(), start.elapsed());
                    }
                } else {
                    // Standard path: Serialize AST once for all global rules
                    let ast_raw = Self::to_raw_value(&ast, "AST")?;

                    for rule in global_rule_names {
                        let start = Instant::now();
                        match host.run_rule_with_parts(
                            &rule,
                            &ast_raw,
                            &content,
                            &tokens,
                            &sentences,
                            path.to_str(),
                        ) {
                            Ok(diags) => global_diagnostics.extend(diags),
                            Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                        }
                        if self.config.timings {
                            timings.insert(rule, start.elapsed());
                        }
                    }
                }
            }

            // B. Run Block Rules on CHANGED/NEW blocks
            let block_rule_names = self.get_rule_names_by_isolation(host, IsolationLevel::Block);
            if !block_rule_names.is_empty() {
                let single_block_rule = block_rule_names.len() == 1;
                // Collect AST nodes for changed blocks
                // We map `matched_mask` back to actual AST nodes by traversing.
                let mut block_index = 0;
                self.visit_blocks(&ast, &mut |node| {
                    if block_index < matched_mask.len() {
                        if !matched_mask[block_index] {
                            // This block changed. Run block rules on it.
                            if single_block_rule {
                                // Optimized path for single rule
                                let rule = &block_rule_names[0];
                                let start = Instant::now();
                                match host.run_rule_with_parts(
                                    rule,
                                    node,
                                    &content,
                                    &tokens,
                                    &sentences,
                                    path.to_str(),
                                ) {
                                    Ok(diags) => block_diagnostics.extend(diags),
                                    Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                                }
                                if self.config.timings {
                                    *timings.entry(rule.clone()).or_insert(Duration::new(0, 0)) +=
                                        start.elapsed();
                                }
                            } else if let Ok(node_raw) = Self::to_raw_value(node, "block node") {
                                for rule in &block_rule_names {
                                    let start = Instant::now();
                                    match host.run_rule_with_parts(
                                        rule,
                                        &node_raw,
                                        &content,
                                        &tokens,
                                        &sentences,
                                        path.to_str(),
                                    ) {
                                        Ok(diags) => block_diagnostics.extend(diags),
                                        Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                                    }
                                    if self.config.timings {
                                        *timings
                                            .entry(rule.clone())
                                            .or_insert(Duration::new(0, 0)) += start.elapsed();
                                    }
                                }
                            } else {
                                warn!("Failed to serialize/create RawValue for block node");
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
                d.message.as_str(),
                d.rule_id.as_str(),
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
        // Use optimized distribution algorithm: O(B+D) + sort O(B log B + D log D)
        // instead of O(B*D). See [Self::distribute_diagnostics] docstring for details.
        let new_blocks =
            Self::distribute_diagnostics(current_blocks, &final_diagnostics, &global_keys);

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
        let capacity = if ast.node_type == NodeType::Document {
            ast.children.len()
        } else {
            0
        };
        let mut blocks = Vec::with_capacity(capacity);

        // Helper to traverse and collect blocks
        self.visit_blocks(ast, &mut |node| {
            // Compute hash for this block
            // Use byte-safe operations to prevent panics
            let start = node.span.start as usize;
            let end = node.span.end as usize;
            let content_bytes = content.as_bytes();

            // Safety check for bounds
            if start <= content_bytes.len() && end <= content_bytes.len() && start <= end {
                // Optimize: try to get slice directly to avoid O(N) UTF-8 validation
                // content.get() checks char boundaries in O(1)
                let hash = if let Some(slice) = content.get(start..end) {
                    CacheManager::hash_content(slice)
                } else {
                    // Fallback for non-char-boundary spans (should be rare)
                    let bytes = &content_bytes[start..end];
                    let block_content = String::from_utf8_lossy(bytes);
                    CacheManager::hash_content(&block_content)
                };

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

    /// Extracts ignore ranges (CodeBlock and Code) from AST.
    fn extract_ignore_ranges(&self, ast: &TxtNode) -> Vec<std::ops::Range<usize>> {
        use std::ops::ControlFlow;
        use tsuzulint_ast::visitor::{VisitResult, Visitor, walk_node};

        struct CodeRangeCollector {
            ranges: Vec<std::ops::Range<usize>>,
        }

        impl<'a> Visitor<'a> for CodeRangeCollector {
            fn visit_code_block(&mut self, node: &TxtNode<'a>) -> VisitResult {
                self.ranges
                    .push(node.span.start as usize..node.span.end as usize);
                ControlFlow::Continue(())
            }

            fn visit_code(&mut self, node: &TxtNode<'a>) -> VisitResult {
                self.ranges
                    .push(node.span.start as usize..node.span.end as usize);
                ControlFlow::Continue(())
            }
        }

        let mut collector = CodeRangeCollector { ranges: Vec::new() };
        let _ = walk_node(&mut collector, ast);
        collector.ranges
    }

    /// Distributes diagnostics to blocks efficiently using a sorted cursor approach.
    ///
    /// This optimization reduces complexity from O(Blocks * Diagnostics) to O(Blocks + Diagnostics)
    /// (plus sorting cost O(B log B + D log D)), which is significant for large files with many blocks.
    fn distribute_diagnostics<'a>(
        mut blocks: Vec<BlockCacheEntry>,
        diagnostics: &[tsuzulint_plugin::Diagnostic],
        global_keys: &HashSet<(u32, u32, &'a str, &'a str)>,
    ) -> Vec<BlockCacheEntry> {
        // Ensure blocks are sorted by start position for the sweep-line algorithm to work correctly
        blocks.sort_by_key(|b| b.span.start);

        // 1. Filter out global diagnostics and create a list of references we can sort
        // We use references to avoid cloning diagnostics during the sort/scan phase
        let mut local_diagnostics: Vec<&tsuzulint_plugin::Diagnostic> = diagnostics
            .iter()
            .filter(|d| {
                let key = (
                    d.span.start,
                    d.span.end,
                    d.message.as_str(),
                    d.rule_id.as_str(),
                );
                !global_keys.contains(&key)
            })
            .collect();

        // 2. Sort diagnostics by start position
        // This allows us to scan through them linearly as we iterate through blocks
        local_diagnostics.sort_by_key(|d| d.span.start);

        let mut diag_idx = 0;

        blocks
            .into_iter()
            .map(|mut block| {
                // Advance cursor past diagnostics that start strictly before this block
                // Since blocks are sorted by start position (from AST traversal),
                // and we've processed previous blocks, any diagnostic starting before
                // the current block's start is either:
                // a) Already assigned to a previous block
                // b) Not contained in any block (e.g. spans across blocks or starts before first block)
                // In either case, we can safely skip it for the current block.
                while diag_idx < local_diagnostics.len()
                    && local_diagnostics[diag_idx].span.start < block.span.start
                {
                    diag_idx += 1;
                }

                let mut block_diags = Vec::new();
                let mut temp_idx = diag_idx;

                // Scan forward from cursor looking for contained diagnostics
                while temp_idx < local_diagnostics.len() {
                    let diag = local_diagnostics[temp_idx];

                    // Optimization: if diagnostic starts after block ends, it can't be in this block.
                    // And since diagnostics are sorted, no subsequent diagnostic can be either.
                    if diag.span.start >= block.span.end {
                        break;
                    }

                    // Check strict inclusion:
                    // start >= block.start is guaranteed by the cursor logic.
                    // We only need to check end <= block.end.
                    if diag.span.end <= block.span.end {
                        block_diags.push(diag.clone());
                    }

                    temp_idx += 1;
                }

                block.diagnostics = block_diags;
                block
            })
            .collect()
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
        // Serialize to string + RawValue to avoid serialization overhead in rules
        let ast_raw = Self::to_raw_value(&ast, "AST")?;

        // Extract ignore ranges (CodeBlock and Code)
        let ignore_ranges = self.extract_ignore_ranges(&ast);

        // Tokenize content
        let tokens = self
            .tokenizer
            .tokenize(content)
            .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?;
        let sentences = SentenceSplitter::split(content, &ignore_ranges);

        // Run rules
        let diagnostics = {
            let mut host = self
                .plugin_host
                .lock()
                .map_err(|_| LinterError::Internal("Plugin host lock poisoned".to_string()))?;
            host.run_all_rules_with_parts(&ast_raw, content, &tokens, &sentences, path.to_str())?
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

    /// Creates a test config that uses a subdirectory of `base` for cache.
    /// This avoids creating a separate TempDir for cache when the test
    /// already has its own TempDir for file layout.
    fn test_config_in(base: &Path) -> LinterConfig {
        let cache_dir = base.join(".cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let mut config = LinterConfig::new();
        config.cache_dir = cache_dir.to_string_lossy().to_string();
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
        let expected_hash = config.hash().unwrap();
        let linter = Linter::new(config).unwrap();

        // Verify that the hash stored in Linter matches the one computed from config
        assert_eq!(linter.config_hash, expected_hash);
    }

    #[test]
    fn test_lint_content_with_simple_rule() {
        // Build the test rule WASM
        // If build fails (e.g. missing target), skip test
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!(
                "Skipping test_lint_content_with_simple_rule: WASM build failed (likely missing wasm32-wasip1 target)"
            );
            return;
        };

        let (config, _temp) = test_config();

        // Create linter
        let linter = Linter::new(config).unwrap();

        // Load the rule dynamically
        linter
            .load_rule(&wasm_path)
            .expect("Failed to load test rule");

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

    #[test]
    fn test_discover_files_respects_exclude() {
        use std::fs;
        use tempfile::tempdir;

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

        // Should find test.md but not excluded.md
        assert!(files.iter().any(|f| f.ends_with("test.md")));
        assert!(
            !files
                .iter()
                .any(|f| f.to_string_lossy().contains("node_modules"))
        );
    }

    #[test]
    fn test_discover_files_respects_include() {
        use std::fs;
        use tempfile::tempdir;

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

        // Should only find .md files
        assert!(files.iter().any(|f| f.ends_with("test.md")));
        assert!(!files.iter().any(|f| f.ends_with("test.txt")));
    }

    #[test]
    fn test_discover_files_deduplicates() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test").unwrap();

        let config = test_config_in(temp_dir.path());
        let linter = Linter::new(config).unwrap();

        // Use same pattern twice
        let files = linter
            .discover_files(&["*.md".to_string(), "*.md".to_string()], temp_dir.path())
            .unwrap();

        // Should only appear once
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_discover_files_invalid_glob() {
        use std::path::Path;
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let result = linter.discover_files(&["[invalid-glob".to_string()], Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_rule_versions_from_host_empty() {
        let host = PluginHost::new();
        let versions = Linter::get_rule_versions_from_host(&host);
        assert!(versions.is_empty());
    }

    #[test]
    fn test_extract_blocks_empty_document() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let (config, _temp) = test_config();
        let _linter = Linter::new(config).unwrap();

        let _arena = AstArena::new();
        let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 0), &[]);

        let blocks = _linter.extract_blocks(&doc, "");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_extract_blocks_with_content() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "Hello"));
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 5),
            arena.alloc_slice_copy(&[*text]),
        ));
        let doc = TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 5),
            arena.alloc_slice_copy(&[*para]),
        );

        let blocks = linter.extract_blocks(&doc, "Hello");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].span.start, 0);
        assert_eq!(blocks[0].span.end, 5);
    }

    #[test]
    fn test_extract_blocks_handles_out_of_bounds_gracefully() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let arena = AstArena::new();
        // Create a node with span beyond content length
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 100), ""));
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 100),
            arena.alloc_slice_copy(&[*text]),
        ));
        let doc = TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 100),
            arena.alloc_slice_copy(&[*para]),
        );

        // Should not panic, just skip invalid blocks
        let blocks = linter.extract_blocks(&doc, "short");
        // The out-of-bounds block should be skipped (warning logged)
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_ast_to_json_structure() {
        use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

        let (config, _temp) = test_config();
        let _linter = Linter::new(config).unwrap();

        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "Hello"));
        let children = arena.alloc_slice_copy(&[*text]);
        let para = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 5), children);

        let json = serde_json::to_value(para).unwrap();

        assert_eq!(json["type"], "Paragraph");
        assert!(json["range"].is_array());
        assert_eq!(json["range"][0], 0);
        assert_eq!(json["range"][1], 5);
        assert!(json["children"].is_array());
        assert_eq!(json["children"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_select_parser_case_insensitive() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let parser_md = linter.select_parser("MD");
        assert_eq!(parser_md.name(), "markdown");

        let parser_txt = linter.select_parser("TXT");
        assert_eq!(parser_txt.name(), "text");
    }

    #[test]
    fn test_linter_with_multiple_patterns() {
        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let patterns = vec!["*.md".to_string(), "*.txt".to_string()];
        let result = linter.lint_patterns(&patterns);

        // Should succeed even if no files match
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_rule_names_by_isolation_empty() {
        use tsuzulint_plugin::IsolationLevel;

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();
        let host = PluginHost::new();

        let global_rules = linter.get_rule_names_by_isolation(&host, IsolationLevel::Global);
        assert!(global_rules.is_empty());

        let block_rules = linter.get_rule_names_by_isolation(&host, IsolationLevel::Block);
        assert!(block_rules.is_empty());
    }

    #[test]
    fn test_lint_content_with_empty_string() {
        use std::path::PathBuf;

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let result = linter.lint_content("", &PathBuf::from("test.md"));
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        // Should handle empty content without errors
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_lint_content_with_unknown_extension() {
        use std::path::PathBuf;

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        // Should default to text parser
        let result = linter.lint_content("Hello", &PathBuf::from("test.xyz"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_lint_file_success() {
        use std::fs;

        let (mut config, temp_dir) = test_config();

        // Enable the rule in config so lint_file runs it (if loaded)
        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule".to_string(),
        ));
        config.options.insert(
            "test-rule".to_string(),
            crate::config::RuleOption::Enabled(true),
        );

        let linter = Linter::new(config).unwrap();

        let mut rule_loaded = false;

        // Build the test rule WASM if available
        if let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() {
            // Load the rule dynamically
            linter
                .load_rule(&wasm_path)
                .expect("Failed to load test rule");
            rule_loaded = true;
        } else {
            println!("WASM build failed, running test without rules");
        }

        // Create a temporary file with content that triggers the rule (if loaded)
        let file_path = temp_dir.path().join("test_lint_file.md");
        let content = "This file contains an error keyword.";
        fs::write(&file_path, content).unwrap();

        // Test lint_file success case
        let result = linter.lint_file(&file_path);

        assert!(result.is_ok(), "lint_file should return Ok");
        let lint_result = result.unwrap();

        // Check path
        assert_eq!(lint_result.path, file_path);

        if rule_loaded {
            // Check diagnostics
            // The simple rule triggers on "error"
            assert_eq!(lint_result.diagnostics.len(), 1, "Should find 1 diagnostic");
            let diag = &lint_result.diagnostics[0];
            assert_eq!(diag.rule_id, "test-rule");
            assert_eq!(diag.message, "Found error keyword");
        } else {
            // Without rules, no diagnostics
            assert!(
                lint_result.diagnostics.is_empty(),
                "No rules loaded, should be clean"
            );
        }

        // Test with clean file
        let clean_path = temp_dir.path().join("clean.md");
        fs::write(&clean_path, "This file is clean.").unwrap();

        let clean_result = linter.lint_file(&clean_path).unwrap();
        assert!(
            clean_result.diagnostics.is_empty(),
            "Clean file should have no diagnostics"
        );
    }

    #[test]
    fn test_resolve_manifest_path_relative() {
        use tempfile::tempdir;
        let temp_dir = tempdir().unwrap();
        let base = temp_dir.path();
        let path = "rule.json";
        // Ensure file exists for canonicalization check
        std::fs::write(base.join(path), "").unwrap();

        // Call private static method via Linter type
        let resolved = Linter::resolve_manifest_path(Some(base), path);
        // Canonicalization resolves symlinks and absolute path, so we compare canonicalized
        assert_eq!(resolved, Some(base.join(path).canonicalize().unwrap()));
    }

    #[test]
    fn test_resolve_manifest_path_traversal_rejected() {
        use std::path::Path;
        let base = Path::new("/tmp/base");
        let path = "../../etc/passwd";
        // This is rejected by lexical check before canonicalization
        let resolved = Linter::resolve_manifest_path(Some(base), path);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_resolve_manifest_path_outside_base_rejected() {
        use tempfile::tempdir;
        let temp_dir = tempdir().unwrap();
        let base = temp_dir.path().join("base");
        std::fs::create_dir(&base).unwrap();

        // Create a file outside base
        let outside_file = temp_dir.path().join("outside.json");
        std::fs::write(&outside_file, "").unwrap();

        // Create a symlink inside base pointing to outside
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link_path = base.join("link.json");
            symlink(&outside_file, &link_path).unwrap();

            // This should be rejected because it resolves outside base
            // pass relative path to link
            let resolved = Linter::resolve_manifest_path(Some(&base), "link.json");
            assert_eq!(resolved, None);
        }
    }

    #[test]
    fn test_resolve_manifest_path_no_base_dir() {
        use std::path::PathBuf;
        // With no base dir, it should return relative path as-is (lexical check still applies)
        let resolved = Linter::resolve_manifest_path(None, "rule.json");
        assert_eq!(resolved, Some(PathBuf::from("rule.json")));
    }

    #[test]
    fn test_resolve_manifest_path_absolute_rejected() {
        use std::path::Path;
        let base = Path::new("/tmp/base");

        #[cfg(unix)]
        let abs_path = "/etc/passwd";
        #[cfg(windows)]
        let abs_path = r"C:\Windows\System32\drivers\etc\hosts";
        #[cfg(not(any(unix, windows)))]
        let abs_path = "/absolute/path";

        let resolved = Linter::resolve_manifest_path(Some(base), abs_path);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_load_rule_absolute_path_security() {
        use std::fs;
        use tempfile::tempdir;

        // Build existing rule WASM
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test: WASM build failed");
            return;
        };

        let temp_dir = tempdir().unwrap();
        let manifest_path = temp_dir.path().join("tsuzulint-rule.json");
        let dest_wasm_path = temp_dir.path().join("rule.wasm");

        // Copy WASM
        fs::copy(&wasm_path, &dest_wasm_path).unwrap();

        // Create manifest
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

        // Get absolute path to manifest
        let abs_manifest_path = manifest_path.canonicalize().unwrap();
        assert!(abs_manifest_path.is_absolute());

        // Configure linter with absolute path
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

        // With security fix, this should be empty (rule skipped).
        // Without fix, it would contain "abs-path-rule".
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

        // Create a temporary file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");
        std::fs::write(&file_path, "content").unwrap();

        // One valid file, one invalid file
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
        // Build the test rule WASM if available
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test_lint_caching: WASM build failed");
            return;
        };

        let (mut config, temp_dir) = test_config();
        // Enable caching
        config.cache = true;

        // Setup rule
        config.rules.push(crate::config::RuleDefinition::Simple(
            "test-rule".to_string(),
        ));
        config.options.insert(
            "test-rule".to_string(),
            crate::config::RuleOption::Enabled(true),
        );

        let linter = Linter::new(config).unwrap();
        linter.load_rule(&wasm_path).expect("Failed to load rule");

        let file_path = temp_dir.path().join("test.md");
        std::fs::write(&file_path, "Clean content").unwrap();

        // First run - should not be cached
        let result1 = linter.lint_file(&file_path).unwrap();
        assert!(!result1.from_cache);

        // Second run - should be cached
        let result2 = linter.lint_file(&file_path).unwrap();
        assert!(result2.from_cache);
    }

    #[test]
    fn test_lint_patterns_expansion() {
        use std::fs;
        let (mut config, temp_dir) = test_config();

        // Set base_dir so lint_patterns searches in temp_dir
        config.base_dir = Some(temp_dir.path().to_path_buf());

        let linter = Linter::new(config).unwrap();

        let dir = temp_dir.path();
        fs::write(dir.join("a.md"), "").unwrap();
        fs::write(dir.join("b.md"), "").unwrap();
        fs::write(dir.join("c.txt"), "").unwrap();

        // Pattern matching .md files in the temp dir
        // We use relative pattern now that base_dir is set
        let pattern = "*.md".to_string();

        let (successes, _failures) = linter.lint_patterns(&[pattern]).unwrap();
        // We expect 2 .md files
        assert_eq!(successes.len(), 2);
    }

    #[test]
    fn test_extract_ignore_ranges() {
        use tsuzulint_ast::AstArena;
        use tsuzulint_parser::{MarkdownParser, Parser};

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();

        let content = "Text.\n```rust\ncode.\n```\nInline `code.` here.";
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let ast = parser.parse(&arena, content).unwrap();

        let ranges = linter.extract_ignore_ranges(&ast);

        // CodeBlock: "```rust\ncode.\n```" (span 6..23)
        // Inline: "`code.`" (span 31..38)

        assert_eq!(ranges.len(), 2, "Expected 2 ignored ranges");

        // Ranges might come in any order depending on traversal, but usually document order.
        let r1 = &ranges[0];
        let r2 = &ranges[1];

        let (block, inline) = if r1.start < r2.start {
            (r1, r2)
        } else {
            (r2, r1)
        };

        let block_text = &content[block.clone()];
        assert!(
            block_text.starts_with("```"),
            "First range should be code block"
        );

        let inline_text = &content[inline.clone()];
        assert_eq!(inline_text, "`code.`", "Second range should be inline code");
    }

    #[test]
    fn test_lint_file_too_large() {
        use std::fs;

        let (config, temp_dir) = test_config();
        let linter = Linter::new(config).unwrap();

        let large_file = temp_dir.path().join("large.txt");
        let file = fs::File::create(&large_file).unwrap();
        // Set size to MAX_FILE_SIZE + 1
        file.set_len(MAX_FILE_SIZE + 1).unwrap();

        let result = linter.lint_file(&large_file);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("File size exceeds limit"));
        assert!(err.contains(&MAX_FILE_SIZE.to_string()));
    }

    #[test]
    fn test_distribute_diagnostics() {
        use tsuzulint_ast::Span;
        use tsuzulint_plugin::Diagnostic;

        let global_keys = HashSet::new();

        // Create 2 disjoint blocks
        let block1 = BlockCacheEntry {
            hash: "b1".to_string(),
            span: Span::new(10, 20),
            diagnostics: vec![],
        };
        let block2 = BlockCacheEntry {
            hash: "b2".to_string(),
            span: Span::new(30, 40),
            diagnostics: vec![],
        };
        let blocks = vec![block1, block2];

        // Create diagnostics
        let diag1 = Diagnostic {
            rule_id: "rule1".to_string(),
            message: "msg1".to_string(),
            span: Span::new(12, 15), // Inside b1
            severity: tsuzulint_plugin::Severity::Error,
            fix: None,
            loc: None,
        };
        let diag2 = Diagnostic {
            rule_id: "rule2".to_string(),
            message: "msg2".to_string(),
            span: Span::new(32, 35), // Inside b2
            severity: tsuzulint_plugin::Severity::Error,
            fix: None,
            loc: None,
        };
        let diag_outside = Diagnostic {
            rule_id: "rule3".to_string(),
            message: "msg3".to_string(),
            span: Span::new(0, 5), // Before blocks
            severity: tsuzulint_plugin::Severity::Error,
            fix: None,
            loc: None,
        };
        let diag_overlap = Diagnostic {
            rule_id: "rule4".to_string(),
            message: "msg4".to_string(),
            span: Span::new(15, 25), // Overlaps b1 (15-20) but ends outside
            severity: tsuzulint_plugin::Severity::Error,
            fix: None,
            loc: None,
        };

        // Note: Diagnostics can be unsorted initially
        let diagnostics = vec![
            diag2.clone(),
            diag1.clone(),
            diag_outside.clone(),
            diag_overlap,
        ];

        let result = Linter::distribute_diagnostics(blocks.clone(), &diagnostics, &global_keys);

        assert_eq!(result.len(), 2);

        // Block 1 should contain diag1
        assert_eq!(result[0].diagnostics.len(), 1);
        assert_eq!(result[0].diagnostics[0].rule_id, "rule1");

        // Block 2 should contain diag2
        assert_eq!(result[1].diagnostics.len(), 1);
        assert_eq!(result[1].diagnostics[0].rule_id, "rule2");

        // Case 2: Filter global diagnostics
        let mut global_keys_filtered = HashSet::new();
        // Mark diag1 as global
        global_keys_filtered.insert((
            diag1.span.start,
            diag1.span.end,
            diag1.message.as_str(),
            diag1.rule_id.as_str(),
        ));

        let result_filtered =
            Linter::distribute_diagnostics(blocks.clone(), &diagnostics, &global_keys_filtered);

        // Block 1 should NOT contain diag1 anymore (filtered out)
        assert!(result_filtered[0].diagnostics.is_empty());

        // Block 2 should still contain diag2
        assert_eq!(result_filtered[1].diagnostics.len(), 1);
        assert_eq!(result_filtered[1].diagnostics[0].rule_id, "rule2");

        // Case 3: Boundary condition – diagnostic at exact block boundary (half-open interval)
        let block_boundary = BlockCacheEntry {
            hash: "bb".to_string(),
            span: Span::new(10, 20),
            diagnostics: vec![],
        };
        let diag_at_end = Diagnostic {
            rule_id: "rule_boundary".to_string(),
            message: "at_end".to_string(),
            span: Span::new(20, 25), // starts exactly at block.end
            severity: tsuzulint_plugin::Severity::Error,
            fix: None,
            loc: None,
        };
        let diag_zero_at_end = Diagnostic {
            rule_id: "rule_zero".to_string(),
            message: "zero_at_end".to_string(),
            span: Span::new(20, 20), // zero-length at block.end
            severity: tsuzulint_plugin::Severity::Error,
            fix: None,
            loc: None,
        };
        let result_boundary = Linter::distribute_diagnostics(
            vec![block_boundary],
            &[diag_at_end, diag_zero_at_end],
            &HashSet::new(),
        );
        // Neither diagnostic should be assigned (half-open: block.end is exclusive)
        assert!(result_boundary[0].diagnostics.is_empty());
    }

    #[test]
    fn test_lint_content_with_special_characters() {
        // This test ensures that special characters (quotes, newlines) are passed correctly
        // to the plugin without double-escaping issues, now that we pass &str directly.

        // Build the test rule WASM if available
        let Some(wasm_path) = crate::test_utils::build_simple_rule_wasm() else {
            println!("Skipping test: WASM build failed");
            return;
        };

        let (config, _temp) = test_config();
        let linter = Linter::new(config).unwrap();
        linter
            .load_rule(&wasm_path)
            .expect("Failed to load test rule");

        // "error" inside quotes. JSON-escaping logic might mess this up if not careful.
        // Original: "This contains \"error\"."
        // If double escaped: "This contains \\\"error\\\"." -> rule might not match "error" if it looks for word boundaries
        // Or if unescaped incorrectly: might crash or parse error.

        let content = "This contains \"error\".";
        let path = Path::new("special.md");

        let diagnostics = linter.lint_content(content, path).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, "test-rule");
        assert_eq!(diagnostics[0].message, "Found error keyword");

        // Verify span is correct (should point to `error`, not `\"error\"`)
        // content: This contains "error".
        // indices: 0123456789012345678901
        // "error" starts at 15, ends at 20.
        // 123456789012345
        // T h i s   c o n t a i n s   " e r r o r " .
        // 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
        // start: 15 ("e"), end: 20 ("r" + 1)
        assert_eq!(diagnostics[0].span.start, 15);
        assert_eq!(diagnostics[0].span.end, 20);
    }
}
