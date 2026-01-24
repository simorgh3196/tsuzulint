//! Core linter engine.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use globset::{Glob, GlobSet, GlobSetBuilder};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use texide_ast::{AstArena, NodeType, TxtNode};
use texide_cache::{entry::BlockCacheEntry, CacheEntry, CacheManager};
use texide_parser::{MarkdownParser, Parser, PlainTextParser};
use texide_plugin::{IsolationLevel, PluginHost};

use crate::{LintResult, LinterConfig, LinterError};

/// The core linter engine.
///
/// Orchestrates file discovery, parsing, rule execution, and caching.
pub struct Linter {
    /// Linter configuration.
    config: LinterConfig,
    /// Plugin host for WASM rules.
    plugin_host: Mutex<PluginHost>,
    /// Cache manager.
    cache: Mutex<CacheManager>,
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

        Ok(Self {
            config,
            plugin_host: Mutex::new(PluginHost::new()),
            cache: Mutex::new(cache),
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
    pub fn load_rule(&self, path: impl AsRef<Path>) -> Result<(), LinterError> {
        let mut host = self.plugin_host.lock().unwrap();
        host.load_rule(path)?;
        Ok(())
    }

    /// Lints files matching the given patterns.
    pub fn lint_patterns(&self, patterns: &[String]) -> Result<Vec<LintResult>, LinterError> {
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

    /// Lints a list of files.
    ///
    /// Note: Currently processes files sequentially. For parallel processing,
    /// parsers need to implement Send + Sync, which requires changes to
    /// the markdown-rs crate's ParseOptions.
    pub fn lint_files(&self, paths: &[PathBuf]) -> Result<Vec<LintResult>, LinterError> {
        let mut results = Vec::with_capacity(paths.len());

        for path in paths {
            match self.lint_file(path) {
                Ok(result) => results.push(result),
                Err(e) => {
                    warn!("Failed to lint {}: {}", path.display(), e);
                }
            }
        }

        // Save cache
        if let Err(e) = self.cache.lock().unwrap().save() {
            warn!("Failed to save cache: {}", e);
        }

        Ok(results)
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

    /// Lints a single file.
    fn lint_file(&self, path: &Path) -> Result<LintResult, LinterError> {
        debug!("Linting {}", path.display());

        // Read file content
        let content = fs::read_to_string(path)
            .map_err(|e| LinterError::file(format!("Failed to read {}: {}", path.display(), e)))?;

        let content_hash = CacheManager::hash_content(&content);
        let config_hash = self.config.hash();
        let rule_versions = self.get_rule_versions();

        // 1. Check full cache first
        {
            let cache = self.cache.lock().unwrap();
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
            let cache = self.cache.lock().unwrap();
            cache.reconcile_blocks(path, &current_blocks, &config_hash, &rule_versions)
        };

        // Prepare diagnostics collection
        let mut final_diagnostics = reused_diagnostics;

        // Run rules
        {
            let mut host = self.plugin_host.lock().unwrap();

            // A. Run Global Rules
            // Global rules must always run on the full document if anything changed
            // because they depend on the full context.
            // (Optimization: If we knew WHICH part changed, maybe we could skip?
            // But 'Global' implies full dependency, so safe bet is re-run).
            let global_rule_names = self.get_rule_names_by_isolation(&host, IsolationLevel::Global);
            if !global_rule_names.is_empty() {
                let ast_json = self.ast_to_json(&ast, &content);
                for rule in global_rule_names {
                     match host.run_rule(&rule, &ast_json, &content, path.to_str()) {
                         Ok(diags) => final_diagnostics.extend(diags),
                         Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                     }
                }
            }

            // B. Run Block Rules on CHANGED/NEW blocks
            let block_rule_names = self.get_rule_names_by_isolation(&host, IsolationLevel::Block);
            if !block_rule_names.is_empty() {
                // Collect AST nodes for changed blocks
                // We need to map blocks back to AST nodes.
                // Since `current_blocks` are derived from AST, we can traverse AST and check spans.
                // However, `extract_blocks` creates flat list.
                // Better: iterate AST, find nodes corresponding to blocks where matched_mask is false.

                // For simplicity/performance in this MVP:
                // We just traverse the AST again. If a node corresponds to a Block (Paragraph, Header, etc.),
                // we check if it was matched.
                // Problem: `extract_blocks` flattens. Matching back to AST node might be tricky if not careful.
                // Solution: We'll construct a list of "nodes to lint" based on `matched_mask`.

                // Let's iterate `current_blocks` alongside `matched_mask`.
                // If !matched, we need to run block rules on THIS block's content/node.
                // To do this efficiently, we probably want `extract_blocks` to return references to AST nodes?
                // But `BlockCacheEntry` needs to be serializable (no AST refs).

                // Let's change approach slightly:
                // We can just run rules on the specific AST sub-trees that correspond to changed blocks.
                // But `host.run_rule` expects JSON.

                // Hack: We can serialize just the relevant sub-tree to JSON and run the rule on it.
                // BUT: The rule expects `TxtNode`.
                // Does the rule need to know it's a root or a fragment?
                // Most rules iterate over children. Passing a Paragraph node as root should work for block rules
                // if they are written to handle visiting that node type.

                // Limitation: If a rule expects "Document" as root, passing "Paragraph" might fail?
                // `texide_plugin` traversing logic usually starts at root.
                // If we pass a fragment, the rule will visit that fragment and children.
                // As long as the rule doesn't assume root is Document, it's fine.
                // `IsolationLevel::Block` rules SHOULD be designed to handle fragments.

                // Optimization: We need to map `matched_mask` back to actual AST nodes.
                // Since `extract_blocks` traverses in a deterministic order, we can replicate that traversal.

                let mut block_index = 0;
                self.visit_blocks(&ast, &mut |node| {
                    if block_index < matched_mask.len() {
                        if !matched_mask[block_index] {
                            // This block changed. Run block rules on it.
                            let node_json = self.ast_to_json(node, &content);
                            for rule in &block_rule_names {
                                match host.run_rule(rule, &node_json, &content, path.to_str()) {
                                    Ok(diags) => final_diagnostics.extend(diags),
                                    Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                                }
                            }
                        }
                        block_index += 1;
                    }
                });
            }
        }

        // Remove duplicates if any (e.g. from overlapping runs or global rules reporting same thing)
        // (Optional but good for safety)

        // Update cache
        // We need to associate diagnostics with blocks for NEXT time.
        // This is tricky: we have `final_diagnostics`. Which diagnostic belongs to which block?
        // We need to re-partition diagnostics into blocks to store in `BlockCacheEntry`.

        let new_blocks: Vec<BlockCacheEntry> = current_blocks.into_iter().map(|mut block| {
            // Filter diagnostics that fall within this block's span
            let block_diags: Vec<_> = final_diagnostics.iter()
                .filter(|d| d.span.start >= block.span.start && d.span.end <= block.span.end)
                .cloned()
                .collect();
            block.diagnostics = block_diags;
            block
        }).collect();

        {
            let mut cache = self.cache.lock().unwrap();
            let entry = CacheEntry::new(
                content_hash,
                config_hash,
                rule_versions,
                final_diagnostics.clone(),
                new_blocks,
            );
            cache.set(path.to_path_buf(), entry);
        }

        Ok(LintResult::new(path.to_path_buf(), final_diagnostics))
    }

    /// Extracts blocks from AST for caching.
    fn extract_blocks(&self, ast: &TxtNode, content: &str) -> Vec<BlockCacheEntry> {
        let mut blocks = Vec::new();

        // Helper to traverse and collect blocks
        self.visit_blocks(ast, &mut |node| {
            // Compute hash for this block
            // For now, simple content hash of the span
            let block_content = &content[node.span.start as usize..node.span.end as usize];
            let hash = CacheManager::hash_content(block_content);

            blocks.push(BlockCacheEntry {
                hash,
                span: node.span,
                diagnostics: Vec::new(), // Will be populated later
            });
        });

        blocks
    }

    /// Visits nodes that are considered "blocks" (e.g. Paragraphs, Headers).
    /// This defines the granularity of incremental caching.
    fn visit_blocks<F>(&self, node: &TxtNode, f: &mut F)
    where F: FnMut(&TxtNode)
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
    fn get_rule_names_by_isolation(&self, host: &PluginHost, level: IsolationLevel) -> Vec<String> {
        let mut names = Vec::new();
        for name in host.loaded_rules() {
            if let Some(manifest) = host.get_manifest(name) {
                if manifest.isolation_level == level {
                    names.push(name.to_string());
                }
            }
        }
        names
    }

    /// Gets the versions of all loaded rules.
    fn get_rule_versions(&self) -> HashMap<String, String> {
        let host = self.plugin_host.lock().unwrap();
        let mut versions = HashMap::new();

        for name in host.loaded_rules() {
            if let Some(manifest) = host.get_manifest(name) {
                versions.insert(name.to_string(), manifest.version.clone());
            }
        }

        versions
    }

    /// Converts a TxtNode to JSON for the plugin system.
    fn ast_to_json(&self, node: &texide_ast::TxtNode, _source: &str) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linter_new() {
        let config = LinterConfig::new();
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
        let mut config = LinterConfig::new();
        config.cache = false;

        let linter = Linter::new(config).unwrap();
        // Verify linter was created successfully with cache disabled
        assert!(linter.include_globs.is_none());
    }

    #[test]
    fn test_linter_with_include_patterns() {
        let mut config = LinterConfig::new();
        config.include = vec!["**/*.md".to_string()];

        let linter = Linter::new(config).unwrap();
        assert!(linter.include_globs.is_some());
    }

    #[test]
    fn test_linter_with_exclude_patterns() {
        let mut config = LinterConfig::new();
        config.exclude = vec!["**/node_modules/**".to_string()];

        let linter = Linter::new(config).unwrap();
        assert!(linter.exclude_globs.is_some());
    }

    #[test]
    fn test_linter_select_parser_markdown() {
        let config = LinterConfig::new();
        let linter = Linter::new(config).unwrap();

        let parser = linter.select_parser("md");
        assert_eq!(parser.name(), "markdown");

        let parser = linter.select_parser("markdown");
        assert_eq!(parser.name(), "markdown");
    }

    #[test]
    fn test_linter_select_parser_text() {
        let config = LinterConfig::new();
        let linter = Linter::new(config).unwrap();

        let parser = linter.select_parser("txt");
        assert_eq!(parser.name(), "text");

        let parser = linter.select_parser("text");
        assert_eq!(parser.name(), "text");
    }

    #[test]
    fn test_linter_select_parser_unknown_defaults_to_text() {
        let config = LinterConfig::new();
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
        use texide_ast::{AstArena, NodeType, Span, TxtNode};

        let config = LinterConfig::new();
        let linter = Linter::new(config).unwrap();

        let arena = AstArena::new();
        let text_node = arena.alloc(TxtNode::new_text(
            NodeType::Str,
            Span::new(0, 5),
            "hello",
        ));
        let children = arena.alloc_slice_copy(&[*text_node]);
        let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 5), children);

        let json = linter.ast_to_json(&doc, "hello");

        assert_eq!(json["type"], "Document");
        assert!(json["range"].is_array());
        assert!(json["children"].is_array());
    }
}
