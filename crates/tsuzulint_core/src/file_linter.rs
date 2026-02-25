//! Single file linting logic.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use tsuzulint_ast::AstArena;
use tsuzulint_cache::CacheManager;
use tsuzulint_parser::{MarkdownParser, Parser, PlainTextParser};
use tsuzulint_plugin::{IsolationLevel, PluginHost, RuleManifest};
use tsuzulint_text::{SentenceSplitter, Tokenizer};

use crate::block_extractor::{extract_blocks, visit_blocks};
use crate::diagnostic_dist::distribute_diagnostics;
use crate::error::LinterError;
use crate::ignore_range::extract_ignore_ranges;
use crate::result::LintResult;

pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

pub fn lint_file_internal(
    path: &Path,
    host: &mut PluginHost,
    tokenizer: &Arc<Tokenizer>,
    config_hash: &str,
    cache: &Mutex<CacheManager>,
    enabled_rules: &HashSet<&str>,
    timings_enabled: bool,
) -> Result<LintResult, LinterError> {
    debug!("Linting {}", path.display());

    let metadata = fs::metadata(path).map_err(|e| {
        LinterError::file(format!(
            "Failed to read metadata for {}: {}",
            path.display(),
            e
        ))
    })?;

    if !metadata.is_file() {
        return Err(LinterError::file(format!(
            "Not a regular file: {}",
            path.display()
        )));
    }

    if metadata.len() > MAX_FILE_SIZE {
        return Err(LinterError::file(format!(
            "File size exceeds limit of {} bytes: {}",
            MAX_FILE_SIZE,
            path.display()
        )));
    }

    let content = fs::read_to_string(path)
        .map_err(|e| LinterError::file(format!("Failed to read {}: {}", path.display(), e)))?;

    let content_hash = CacheManager::hash_content(&content);
    let rule_versions = super::rule_loader::get_rule_versions_from_host(host);

    {
        let cache_guard = cache
            .lock()
            .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
        if cache_guard.is_valid(path, &content_hash, config_hash, &rule_versions)
            && let Some(entry) = cache_guard.get(path)
        {
            debug!("Using cached result for {}", path.display());
            return Ok(LintResult::cached(
                path.to_path_buf(),
                entry.diagnostics.clone(),
            ));
        }
    }

    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parser = select_parser(extension);

    let arena = AstArena::new();
    let ast = parser
        .parse(&arena, &content)
        .map_err(|e| LinterError::parse(e.to_string()))?;

    let ignore_ranges = extract_ignore_ranges(&ast);

    let (global_rule_names, block_rule_names, needs_morphology) =
        get_classified_rules(host.loaded_rules(), host, enabled_rules);

    let tokens = if needs_morphology {
        tokenizer
            .tokenize(&content)
            .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?
    } else {
        Vec::new()
    };
    let sentences = SentenceSplitter::split(&content, &ignore_ranges);

    let current_blocks = extract_blocks(&ast, &content);

    let (reused_diagnostics, matched_mask) = {
        let cache_guard = cache
            .lock()
            .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
        cache_guard.reconcile_blocks(path, &current_blocks, config_hash, &rule_versions)
    };

    let mut global_diagnostics = Vec::new();
    let mut block_diagnostics = Vec::new();
    let mut timings = HashMap::new();

    {
        if !global_rule_names.is_empty() {
            if global_rule_names.len() == 1 {
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
                if timings_enabled {
                    *timings.entry(rule.clone()).or_insert(Duration::ZERO) += start.elapsed();
                }
            } else {
                let ast_raw = to_raw_value(&ast, "AST")?;
                let request_bytes = host
                    .prepare_lint_request(&ast_raw, &content, &tokens, &sentences, path.to_str())
                    .map_err(|e| {
                        LinterError::Internal(format!("Failed to prepare lint request: {}", e))
                    })?;

                for rule in &global_rule_names {
                    let start = Instant::now();
                    match host.run_rule_with_prepared(rule, &request_bytes) {
                        Ok(diags) => global_diagnostics.extend(diags),
                        Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                    }
                    if timings_enabled {
                        *timings.entry(rule.clone()).or_insert(Duration::ZERO) += start.elapsed();
                    }
                }
            }
        }

        if !block_rule_names.is_empty() {
            let single_block_rule = block_rule_names.len() == 1;
            let mut block_index = 0;
            visit_blocks(&ast, &mut |node| {
                if block_index < matched_mask.len() {
                    if !matched_mask[block_index] {
                        if single_block_rule {
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
                            if timings_enabled {
                                *timings.entry(rule.clone()).or_insert(Duration::ZERO) +=
                                    start.elapsed();
                            }
                        } else if let Ok(node_raw) = to_raw_value(node, "block node") {
                            // Serialize request once for all rules running on this block
                            match host.prepare_lint_request(
                                &node_raw,
                                &content,
                                &tokens,
                                &sentences,
                                path.to_str(),
                            ) {
                                Ok(request_bytes) => {
                                    for rule in &block_rule_names {
                                        let start = Instant::now();
                                        match host.run_rule_with_prepared(rule, &request_bytes) {
                                            Ok(diags) => block_diagnostics.extend(diags),
                                            Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                                        }
                                        if timings_enabled {
                                            *timings
                                                .entry(rule.clone())
                                                .or_insert(Duration::ZERO) += start.elapsed();
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to prepare lint request for block node: {}", e)
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

    let mut local_diagnostics = reused_diagnostics;
    local_diagnostics.extend(block_diagnostics);

    // Filter out diagnostics that are covered by global rules (by checking rule ID).
    // Global rules take precedence, and their diagnostics should not be stored in block cache.
    // This is faster than hashing entire Diagnostic objects.
    let global_rule_ids: HashSet<&str> = global_rule_names.iter().map(|s| s.as_str()).collect();

    filter_overridden_diagnostics(&mut local_diagnostics, &global_rule_ids);

    local_diagnostics.sort_unstable();
    local_diagnostics.dedup();

    // Distribute local diagnostics to blocks.
    // We pass an empty set for global_keys because we already filtered them out from local_diagnostics.
    let new_blocks = distribute_diagnostics(current_blocks, &local_diagnostics, &HashSet::new());

    let mut final_diagnostics = local_diagnostics;
    final_diagnostics.reserve(global_diagnostics.len());
    final_diagnostics.extend(global_diagnostics);

    final_diagnostics.sort_unstable();
    final_diagnostics.dedup();

    {
        let mut cache_guard = cache
            .lock()
            .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
        let entry = tsuzulint_cache::CacheEntry::new(
            content_hash,
            config_hash.to_string(),
            rule_versions,
            final_diagnostics.clone(),
            new_blocks,
        );
        cache_guard.set(path.to_path_buf(), entry);
    }

    let mut result = LintResult::new(path.to_path_buf(), final_diagnostics);
    result.timings = timings;
    Ok(result)
}

pub fn lint_content(
    content: &str,
    path: &Path,
    host: &mut PluginHost,
    tokenizer: &Arc<Tokenizer>,
) -> Result<Vec<tsuzulint_plugin::Diagnostic>, LinterError> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parser = select_parser(extension);

    let arena = AstArena::new();
    let ast = parser
        .parse(&arena, content)
        .map_err(|e| LinterError::parse(e.to_string()))?;

    let ast_raw = to_raw_value(&ast, "AST")?;

    let ignore_ranges = extract_ignore_ranges(&ast);

    let needs_morphology = host
        .loaded_rules()
        .filter_map(|name| host.get_manifest(name))
        .any(|m| m.needs_morphology());

    let tokens = if needs_morphology {
        tokenizer
            .tokenize(content)
            .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?
    } else {
        Vec::new()
    };
    let sentences = SentenceSplitter::split(content, &ignore_ranges);

    let diagnostics =
        host.run_all_rules_with_parts(&ast_raw, content, &tokens, &sentences, path.to_str())?;

    Ok(diagnostics)
}

/// Enum wrapper for parsers to avoid heap allocation (Box<dyn Parser>) and dynamic dispatch.
/// This improves performance in the hot path of file linting.
enum FileParser {
    Markdown(MarkdownParser),
    Text(PlainTextParser),
}

impl Parser for FileParser {
    fn name(&self) -> &str {
        match self {
            Self::Markdown(p) => p.name(),
            Self::Text(p) => p.name(),
        }
    }

    fn extensions(&self) -> &[&str] {
        match self {
            Self::Markdown(p) => p.extensions(),
            Self::Text(p) => p.extensions(),
        }
    }

    fn parse<'a>(
        &self,
        arena: &'a AstArena,
        source: &str,
    ) -> Result<tsuzulint_ast::TxtNode<'a>, tsuzulint_parser::ParseError> {
        match self {
            Self::Markdown(p) => p.parse(arena, source),
            Self::Text(p) => p.parse(arena, source),
        }
    }
}

fn select_parser(extension: &str) -> FileParser {
    if MarkdownParser::supports_extension(extension) {
        FileParser::Markdown(MarkdownParser::new())
    } else {
        FileParser::Text(PlainTextParser::new())
    }
}

fn to_raw_value<T: serde::Serialize>(
    value: &T,
    label: &str,
) -> Result<Box<serde_json::value::RawValue>, LinterError> {
    serde_json::value::to_raw_value(value)
        .map_err(|e| LinterError::Internal(format!("Failed to serialize {}: {}", label, e)))
}

trait ManifestProvider {
    fn get_manifest(&self, name: &str) -> Option<&RuleManifest>;
}

impl ManifestProvider for PluginHost {
    fn get_manifest(&self, name: &str) -> Option<&RuleManifest> {
        self.get_manifest(name)
    }
}

fn get_classified_rules<'a, I, P>(
    rules: I,
    host: &P,
    enabled_rules: &HashSet<&str>,
) -> (Vec<String>, Vec<String>, bool)
where
    I: Iterator<Item = &'a String>,
    P: ManifestProvider,
{
    let mut global_rules = Vec::new();
    let mut block_rules = Vec::new();
    let mut needs_morphology = false;

    for name in rules {
        if !enabled_rules.contains(name.as_str()) {
            continue;
        }
        if let Some(manifest) = host.get_manifest(name) {
            match manifest.isolation_level {
                IsolationLevel::Global => global_rules.push(name.clone()),
                IsolationLevel::Block => block_rules.push(name.clone()),
            }
            if !needs_morphology && manifest.needs_morphology() {
                needs_morphology = true;
            }
        } else {
            warn!("Rule '{}' is enabled but has no manifest; skipping", name);
        }
    }

    (global_rules, block_rules, needs_morphology)
}

fn filter_overridden_diagnostics(
    local_diagnostics: &mut Vec<tsuzulint_plugin::Diagnostic>,
    global_rule_ids: &HashSet<&str>,
) {
    if !global_rule_ids.is_empty() {
        local_diagnostics.retain(|d| !global_rule_ids.contains(d.rule_id.as_str()));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::Span;
    use tsuzulint_plugin::Diagnostic;

    struct MockManifestProvider {
        manifests: HashMap<String, RuleManifest>,
    }

    impl ManifestProvider for MockManifestProvider {
        fn get_manifest(&self, name: &str) -> Option<&RuleManifest> {
            self.manifests.get(name)
        }
    }

    fn create_manifest(level: IsolationLevel) -> RuleManifest {
        RuleManifest {
            name: "test-rule".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            fixable: false,
            node_types: vec![],
            isolation_level: level,
            schema: None,
            languages: vec![],
            capabilities: vec![],
        }
    }

    #[test]
    fn test_get_classified_rules() {
        let global_name = "global-rule";
        let block_name = "block-rule";
        let disabled_name = "disabled-rule";
        let missing_manifest_name = "missing-manifest-rule";

        let mut manifests = HashMap::new();
        manifests.insert(
            global_name.to_string(),
            create_manifest(IsolationLevel::Global),
        );
        manifests.insert(
            block_name.to_string(),
            create_manifest(IsolationLevel::Block),
        );
        manifests.insert(
            disabled_name.to_string(),
            create_manifest(IsolationLevel::Global),
        );

        let provider = MockManifestProvider { manifests };

        let loaded_rules = [
            global_name.to_string(),
            block_name.to_string(),
            disabled_name.to_string(),
            missing_manifest_name.to_string(),
        ];

        let mut enabled_rules = HashSet::new();
        enabled_rules.insert(global_name);
        enabled_rules.insert(block_name);
        enabled_rules.insert(missing_manifest_name); // Enabled but no manifest

        let (global_rules, block_rules, needs_morphology) =
            get_classified_rules(loaded_rules.iter(), &provider, &enabled_rules);

        assert!(!needs_morphology);
        assert!(global_rules.contains(&global_name.to_string()));
        assert!(!global_rules.contains(&disabled_name.to_string()));
        assert!(!global_rules.contains(&missing_manifest_name.to_string()));

        assert!(block_rules.contains(&block_name.to_string()));

        assert_eq!(global_rules.len(), 1);
        assert_eq!(block_rules.len(), 1);
    }

    #[test]
    fn test_get_classified_rules_with_morphology() {
        use tsuzulint_plugin::Capability;

        let rule_name = "morph-rule";
        let mut manifests = HashMap::new();
        let mut manifest = create_manifest(IsolationLevel::Global);
        manifest.capabilities.push(Capability::Morphology);
        manifests.insert(rule_name.to_string(), manifest);

        let provider = MockManifestProvider { manifests };
        let loaded_rules = [rule_name.to_string()];
        let mut enabled_rules = HashSet::new();
        enabled_rules.insert(rule_name);

        let (_, _, needs_morphology) =
            get_classified_rules(loaded_rules.iter(), &provider, &enabled_rules);

        assert!(needs_morphology);
    }

    #[test]
    fn test_filter_overridden_diagnostics() {
        let global_id = "global-rule";
        let local_id = "local-rule";

        let mut diagnostics = vec![
            Diagnostic::new(global_id, "Global msg", Span::new(0, 10)),
            Diagnostic::new(local_id, "Local msg", Span::new(20, 30)),
        ];

        let mut global_rule_ids = HashSet::new();
        global_rule_ids.insert(global_id);

        filter_overridden_diagnostics(&mut diagnostics, &global_rule_ids);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, local_id);
    }

    #[test]
    fn test_filter_overridden_diagnostics_empty_global_set() {
        let mut diagnostics = vec![Diagnostic::new("rule", "msg", Span::new(0, 10))];
        let global_rule_ids = HashSet::new();

        filter_overridden_diagnostics(&mut diagnostics, &global_rule_ids);

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_filter_overridden_diagnostics_no_match() {
        let mut diagnostics = vec![Diagnostic::new("local-rule", "msg", Span::new(0, 10))];
        let mut global_rule_ids = HashSet::new();
        global_rule_ids.insert("other-global-rule");

        filter_overridden_diagnostics(&mut diagnostics, &global_rule_ids);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, "local-rule");
    }

    #[test]
    fn test_select_parser_markdown() {
        let parser = select_parser("md");
        assert_eq!(parser.name(), "markdown");

        let parser = select_parser("markdown");
        assert_eq!(parser.name(), "markdown");
    }

    #[test]
    fn test_select_parser_text() {
        let parser = select_parser("txt");
        assert_eq!(parser.name(), "text");

        let parser = select_parser("text");
        assert_eq!(parser.name(), "text");
    }

    #[test]
    fn test_select_parser_unknown_defaults_to_text() {
        let parser = select_parser("unknown");
        assert_eq!(parser.name(), "text");
    }

    #[test]
    fn test_select_parser_case_insensitive() {
        let parser_md = select_parser("MD");
        assert_eq!(parser_md.name(), "markdown");

        let parser_txt = select_parser("TXT");
        assert_eq!(parser_txt.name(), "text");
    }

    #[test]
    fn test_file_parser_markdown_parse_returns_ast() {
        let parser = select_parser("md");
        let arena = AstArena::new();
        let result = parser.parse(&arena, "# Hello");
        assert!(result.is_ok());
    }

    #[test]
    fn test_file_parser_text_parse_returns_ast() {
        let parser = select_parser("txt");
        let arena = AstArena::new();
        let result = parser.parse(&arena, "Hello world");
        assert!(result.is_ok());
    }

    #[test]
    fn test_file_parser_markdown_extensions_contains_md() {
        let parser = select_parser("md");
        assert!(parser.extensions().contains(&"md"));
    }
}
