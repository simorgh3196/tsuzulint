//! Single file linting logic.

use std::collections::{HashMap, HashSet};
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
use crate::safe_io::{
    check_file_metadata, clear_nonblocking, handle_io_err, open_nonblocking, read_to_string_bounded,
};

pub const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Context for linting a single file.
pub struct LintContext<'a> {
    pub path: &'a Path,
    pub host: &'a mut PluginHost,
    pub tokenizer: &'a Arc<Tokenizer>,
    pub config_hash: &'a [u8; 32],
    pub cache: &'a Mutex<CacheManager>,
    pub enabled_rules: &'a HashSet<&'a str>,
    pub rule_versions: &'a HashMap<String, String>,
    pub timings_enabled: bool,
    /// Options for enabled rules keyed by rule alias. Used by the native
    /// rule engine to hand per-rule config to rules (the WASM host path
    /// wires options through its own `configure_rule` mechanism).
    pub rule_options: &'a HashMap<String, serde_json::Value>,
}

pub fn lint_file_internal(ctx: &mut LintContext<'_>) -> Result<LintResult, LinterError> {
    let LintContext {
        path,
        host,
        tokenizer,
        config_hash,
        cache,
        enabled_rules,
        rule_versions,
        timings_enabled,
        rule_options,
    } = ctx;
    let timings_enabled = *timings_enabled;
    debug!("Linting {}", path.display());

    // Open with O_NONBLOCK so that opening a FIFO (or other special file)
    // does not block. This eliminates the TOCTOU window between a metadata
    // check and the actual open call.
    let mut file = handle_io_err(open_nonblocking(path), path, "Failed to open")?;

    // Verify the opened fd refers to a regular file (TOCTOU-safe, uses fstat).
    let metadata = handle_io_err(file.metadata(), path, "Failed to read metadata for")?;
    check_file_metadata(&metadata, MAX_FILE_SIZE, path)?;

    // Clear O_NONBLOCK so that subsequent reads block normally.
    handle_io_err(
        clear_nonblocking(&file),
        path,
        "Failed to clear O_NONBLOCK on",
    )?;

    // Bounded read: caps content at MAX_FILE_SIZE even when metadata.len()
    // underreports the file size (e.g. pseudo-files under /proc, /dev/zero).
    let content = read_to_string_bounded(&mut file, MAX_FILE_SIZE, path)?;

    let content_hash = CacheManager::hash_content(&content);

    // Optimization: rule_versions is passed in by reference rather than calling
    // super::rule_loader::get_rule_versions_from_host(host) here to avoid
    // an unnecessary HashMap allocation and insertions for every file being linted.
    {
        let cache_guard = cache
            .lock()
            .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
        if cache_guard.is_valid(path, &content_hash, config_hash, rule_versions)
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

    // host is &mut &'a mut PluginHost after destructuring LintContext.
    // We double-dereference and take an immutable borrow to pass &PluginHost to read-only helpers.
    let wasm_needs_morphology =
        any_rule_needs_morphology(host.loaded_rules(), &**host, enabled_rules);
    let wasm_needs_sentences =
        any_rule_needs_sentences(host.loaded_rules(), &**host, enabled_rules);
    // Native rules also declare capability needs. Without this, a native rule
    // that sets `needs_morphology()` would silently get an empty token slice.
    let registry = crate::native_rules::builtin_registry();
    let native_needs_morphology = enabled_rules
        .iter()
        .any(|name| registry.get(name).is_some_and(|r| r.needs_morphology()));
    let native_needs_sentences = enabled_rules
        .iter()
        .any(|name| registry.get(name).is_some_and(|r| r.needs_sentences()));
    let needs_morphology = wasm_needs_morphology || native_needs_morphology;
    let needs_sentences = wasm_needs_sentences || native_needs_sentences;

    let tokens = if needs_morphology {
        tokenizer
            .tokenize(&content)
            .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?
    } else {
        Vec::new()
    };
    let sentences = if needs_sentences {
        SentenceSplitter::split(&content, &ignore_ranges)
    } else {
        Vec::new()
    };

    let current_blocks = extract_blocks(&ast, &content);

    let (reused_diagnostics, matched_mask) = {
        let cache_guard = cache
            .lock()
            .map_err(|_| LinterError::Internal("Cache mutex poisoned".to_string()))?;
        cache_guard.reconcile_blocks(path, &current_blocks, config_hash, rule_versions)
    };

    let (global_rule_names, block_rule_names) = get_classified_rules(&**host, enabled_rules);

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
                    *timings.entry(rule.to_string()).or_insert(Duration::ZERO) += start.elapsed();
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
                        *timings.entry(rule.to_string()).or_insert(Duration::ZERO) +=
                            start.elapsed();
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
                                *timings.entry(rule.to_string()).or_insert(Duration::ZERO) +=
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
                                                .entry(rule.to_string())
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

    // Native rules run after WASM rules so they see the same pre-computed
    // tokens/sentences and share the ignore_ranges-aware splitter output.
    // We only dispatch native rules whose name appears in enabled_rules AND
    // is not already loaded as a WASM rule — that lets a user override a
    // built-in rule with a custom WASM plugin just by loading one with the
    // same name.
    let mut native_diagnostics: Vec<tsuzulint_plugin::Diagnostic> = Vec::new();
    let registry = crate::native_rules::builtin_registry();
    let null_options = serde_json::Value::Null;
    for &rule_name in enabled_rules.iter() {
        if host.loaded_rules().any(|loaded| loaded == rule_name) {
            continue;
        }
        let Some(rule) = registry.get(rule_name) else {
            continue;
        };
        let options = rule_options.get(rule_name).unwrap_or(&null_options);
        let native_ctx = crate::native_rules::RuleContext {
            ast: &ast,
            source: &content,
            tokens: &tokens,
            sentences: &sentences,
            options,
            file_path: Some(path),
        };
        let start = Instant::now();
        native_diagnostics.extend(rule.lint(&native_ctx));
        if timings_enabled {
            *timings
                .entry(rule_name.to_string())
                .or_insert(Duration::ZERO) += start.elapsed();
        }
    }

    let mut local_diagnostics = reused_diagnostics;
    local_diagnostics.extend(block_diagnostics);
    local_diagnostics.extend(native_diagnostics);

    // Filter out diagnostics that are covered by global rules (by checking rule ID).
    // Global rules take precedence, and their diagnostics should not be stored in block cache.
    // We use binary_search on the already-sorted global_rule_names slice to avoid per-file HashSet allocations.
    filter_overridden_diagnostics(&mut local_diagnostics, &global_rule_names);

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
            **config_hash,
            rule_versions.clone(),
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

    let needs_morphology =
        any_rule_has_capability(host.loaded_rules(), host, None, |m| m.needs_morphology());

    let needs_sentences =
        any_rule_has_capability(host.loaded_rules(), host, None, |m| m.needs_sentences());

    let tokens = if needs_morphology {
        tokenizer
            .tokenize(content)
            .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?
    } else {
        Vec::new()
    };
    let sentences = if needs_sentences {
        SentenceSplitter::split(content, &ignore_ranges)
    } else {
        Vec::new()
    };

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

/// Classifies enabled rules into global and block rules.
///
/// This function iterates over `enabled_rules` (O(M)) instead of all loaded rules (O(N))
/// to optimize performance when M << N. It sorts the results to ensure deterministic
/// output since `enabled_rules` iteration order is arbitrary.
fn get_classified_rules<'a, P>(
    host: &P,
    enabled_rules: &HashSet<&'a str>,
) -> (Vec<&'a str>, Vec<&'a str>)
where
    P: ManifestProvider,
{
    let mut global_rules = Vec::new();
    let mut block_rules = Vec::new();

    for &name in enabled_rules {
        if let Some(manifest) = host.get_manifest(name) {
            match manifest.isolation_level {
                IsolationLevel::Global => global_rules.push(name),
                IsolationLevel::Block => block_rules.push(name),
            }
        } else if crate::native_rules::builtin_registry().get(name).is_some() {
            // Rule is served by the native engine; handled in `lint_file_internal`
            // after the WASM dispatch loop. No warning to avoid noise.
        } else {
            warn!("Missing manifest for enabled rule: {}", name);
        }
    }

    global_rules.sort();
    block_rules.sort();

    (global_rules, block_rules)
}

fn filter_overridden_diagnostics(
    local_diagnostics: &mut Vec<tsuzulint_plugin::Diagnostic>,
    global_rule_names: &[&str],
) {
    if !global_rule_names.is_empty() {
        debug_assert!(global_rule_names.windows(2).all(|w| w[0] <= w[1]));
        local_diagnostics.retain(|d| {
            global_rule_names
                .binary_search(&d.rule_id.as_str())
                .is_err()
        });
    }
}

fn any_rule_has_capability<'a, I, P, F>(
    rules: I,
    host: &P,
    enabled_rules: Option<&HashSet<&str>>,
    predicate: F,
) -> bool
where
    I: Iterator<Item = &'a String>,
    P: ManifestProvider,
    F: Fn(&RuleManifest) -> bool,
{
    for name in rules {
        if enabled_rules.is_some_and(|enabled| !enabled.contains(name.as_str())) {
            continue;
        }
        if host.get_manifest(name).is_some_and(&predicate) {
            return true;
        }
    }
    false
}

fn any_rule_needs_morphology<'a, I, P>(rules: I, host: &P, enabled_rules: &HashSet<&str>) -> bool
where
    I: Iterator<Item = &'a String>,
    P: ManifestProvider,
{
    any_rule_has_capability(rules, host, Some(enabled_rules), |m| m.needs_morphology())
}

fn any_rule_needs_sentences<'a, I, P>(rules: I, host: &P, enabled_rules: &HashSet<&str>) -> bool
where
    I: Iterator<Item = &'a String>,
    P: ManifestProvider,
{
    any_rule_has_capability(rules, host, Some(enabled_rules), |m| m.needs_sentences())
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

    fn create_manifest_with_capabilities(
        capabilities: Vec<tsuzulint_plugin::Capability>,
    ) -> RuleManifest {
        let mut manifest = create_manifest(IsolationLevel::Global);
        manifest.capabilities = capabilities;
        manifest
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

        let mut enabled_rules = HashSet::new();
        enabled_rules.insert(global_name);
        enabled_rules.insert(block_name);
        enabled_rules.insert(missing_manifest_name); // Enabled but no manifest

        let (global_rules, block_rules) = get_classified_rules(&provider, &enabled_rules);

        assert!(global_rules.contains(&global_name));
        assert!(!global_rules.contains(&disabled_name));
        assert!(!global_rules.contains(&missing_manifest_name));

        assert!(block_rules.contains(&block_name));

        assert_eq!(global_rules, vec!["global-rule"]);
        assert_eq!(block_rules, vec!["block-rule"]);
    }

    #[test]
    fn test_get_classified_rules_returns_sorted_output() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "z-global".to_string(),
            create_manifest(IsolationLevel::Global),
        );
        manifests.insert(
            "a-global".to_string(),
            create_manifest(IsolationLevel::Global),
        );
        manifests.insert(
            "z-block".to_string(),
            create_manifest(IsolationLevel::Block),
        );
        manifests.insert(
            "a-block".to_string(),
            create_manifest(IsolationLevel::Block),
        );

        let provider = MockManifestProvider { manifests };
        let enabled_rules = HashSet::from(["z-global", "a-global", "z-block", "a-block"]);

        let (global_rules, block_rules) = get_classified_rules(&provider, &enabled_rules);

        assert_eq!(global_rules, vec!["a-global", "z-global"]);
        assert_eq!(block_rules, vec!["a-block", "z-block"]);
    }

    #[test]
    fn test_filter_overridden_diagnostics() {
        let global_id = "global-rule";
        let local_id = "local-rule";

        let mut diagnostics = vec![
            Diagnostic::new(global_id, "Global msg", Span::new(0, 10)),
            Diagnostic::new(local_id, "Local msg", Span::new(20, 30)),
        ];

        let global_rule_names = vec![global_id];

        filter_overridden_diagnostics(&mut diagnostics, &global_rule_names);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, local_id);
    }

    #[test]
    fn test_filter_overridden_diagnostics_empty_global_set() {
        let mut diagnostics = vec![Diagnostic::new("rule", "msg", Span::new(0, 10))];
        let global_rule_names = vec![];

        filter_overridden_diagnostics(&mut diagnostics, &global_rule_names);

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_filter_overridden_diagnostics_no_match() {
        let mut diagnostics = vec![Diagnostic::new("local-rule", "msg", Span::new(0, 10))];
        let global_rule_names = vec!["other-global-rule"];

        filter_overridden_diagnostics(&mut diagnostics, &global_rule_names);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule_id, "local-rule");
    }

    #[test]
    fn test_any_rule_needs_morphology() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "rule-morphology".to_string(),
            create_manifest_with_capabilities(vec![tsuzulint_plugin::Capability::Morphology]),
        );
        manifests.insert(
            "rule-none".to_string(),
            create_manifest_with_capabilities(vec![]),
        );

        let provider = MockManifestProvider { manifests };

        let mut enabled_rules = HashSet::new();
        enabled_rules.insert("rule-morphology");
        let rules: Vec<String> = vec!["rule-morphology".to_string()];
        assert!(any_rule_needs_morphology(
            rules.iter(),
            &provider,
            &enabled_rules
        ));

        let mut enabled_rules = HashSet::new();
        enabled_rules.insert("rule-none");
        let rules: Vec<String> = vec!["rule-none".to_string()];
        assert!(!any_rule_needs_morphology(
            rules.iter(),
            &provider,
            &enabled_rules
        ));

        let enabled_rules = HashSet::new(); // empty
        let rules: Vec<String> = vec!["rule-morphology".to_string()];
        assert!(!any_rule_needs_morphology(
            rules.iter(),
            &provider,
            &enabled_rules
        ));

        // test rule not in provider
        let mut enabled_rules = HashSet::new();
        enabled_rules.insert("missing");
        let rules: Vec<String> = vec!["missing".to_string()];
        assert!(!any_rule_needs_morphology(
            rules.iter(),
            &provider,
            &enabled_rules
        ));
    }

    #[test]
    fn test_any_rule_needs_sentences() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "rule-sentences".to_string(),
            create_manifest_with_capabilities(vec![tsuzulint_plugin::Capability::Sentences]),
        );
        manifests.insert(
            "rule-none".to_string(),
            create_manifest_with_capabilities(vec![]),
        );

        let provider = MockManifestProvider { manifests };

        let mut enabled_rules = HashSet::new();
        enabled_rules.insert("rule-sentences");
        let rules: Vec<String> = vec!["rule-sentences".to_string()];
        assert!(any_rule_needs_sentences(
            rules.iter(),
            &provider,
            &enabled_rules
        ));

        let mut enabled_rules = HashSet::new();
        enabled_rules.insert("rule-none");
        let rules: Vec<String> = vec!["rule-none".to_string()];
        assert!(!any_rule_needs_sentences(
            rules.iter(),
            &provider,
            &enabled_rules
        ));

        let enabled_rules = HashSet::new(); // empty
        let rules: Vec<String> = vec!["rule-sentences".to_string()];
        assert!(!any_rule_needs_sentences(
            rules.iter(),
            &provider,
            &enabled_rules
        ));

        // test rule not in provider
        let mut enabled_rules = HashSet::new();
        enabled_rules.insert("missing");
        let rules: Vec<String> = vec!["missing".to_string()];
        assert!(!any_rule_needs_sentences(
            rules.iter(),
            &provider,
            &enabled_rules
        ));
    }

    #[test]
    fn test_any_rule_has_capability_no_enabled_rules_filter() {
        let mut manifests = HashMap::new();
        manifests.insert(
            "rule-morphology".to_string(),
            create_manifest_with_capabilities(vec![tsuzulint_plugin::Capability::Morphology]),
        );
        manifests.insert(
            "rule-none".to_string(),
            create_manifest_with_capabilities(vec![]),
        );

        let provider = MockManifestProvider { manifests };

        // Test with a rule that has the capability, passing None for enabled_rules
        let rules: Vec<String> = vec!["rule-morphology".to_string()];
        assert!(any_rule_has_capability(
            rules.iter(),
            &provider,
            None,
            |m| m.needs_morphology()
        ));

        // Test with a rule that doesn't have the capability, passing None for enabled_rules
        let rules: Vec<String> = vec!["rule-none".to_string()];
        assert!(!any_rule_has_capability(
            rules.iter(),
            &provider,
            None,
            |m| m.needs_morphology()
        ));
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
