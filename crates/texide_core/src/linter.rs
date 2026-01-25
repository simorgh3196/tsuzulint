//! Core linter engine.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use globset::{Glob, GlobSet, GlobSetBuilder};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use texide_ast::AstArena;
use texide_cache::{CacheEntry, CacheManager};
use texide_parser::{MarkdownParser, Parser, PlainTextParser};
use texide_plugin::PluginHost;

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

        // Check cache
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

        // Convert AST to JSON for plugin system
        let ast_json = self.ast_to_json(&ast, &content);

        // Run rules
        let diagnostics = {
            let mut host = self.plugin_host.lock().unwrap();
            host.run_all_rules(&ast_json, &content, path.to_str())?
        };

        // Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            let entry = CacheEntry::new(
                content_hash,
                config_hash,
                rule_versions,
                diagnostics.clone(),
            );
            cache.set(path.to_path_buf(), entry);
        }

        Ok(LintResult::new(path.to_path_buf(), diagnostics))
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
        let text_node = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let children = arena.alloc_slice_copy(&[*text_node]);
        let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 5), children);

        let json = linter.ast_to_json(&doc, "hello");

        assert_eq!(json["type"], "Document");
        assert!(json["range"].is_array());
        assert!(json["children"].is_array());
    }
}
