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
use tsuzulint_plugin::{IsolationLevel, PluginHost};
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

    let tokens = tokenizer
        .tokenize(&content)
        .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?;
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
        let global_rule_names =
            get_rule_names_by_isolation(host, enabled_rules, IsolationLevel::Global);
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
                    timings.insert(rule.clone(), start.elapsed());
                }
            } else {
                let ast_raw = to_raw_value(&ast, "AST")?;
                let request_bytes = host
                    .prepare_lint_request(&ast_raw, &content, &tokens, &sentences, path.to_str())
                    .map_err(|e| {
                        LinterError::Internal(format!("Failed to prepare lint request: {}", e))
                    })?;

                for rule in global_rule_names {
                    let start = Instant::now();
                    match host.run_rule_with_prepared(&rule, &request_bytes) {
                        Ok(diags) => global_diagnostics.extend(diags),
                        Err(e) => warn!("Rule '{}' failed: {}", rule, e),
                    }
                    if timings_enabled {
                        timings.insert(rule, start.elapsed());
                    }
                }
            }
        }

        let block_rule_names =
            get_rule_names_by_isolation(host, enabled_rules, IsolationLevel::Block);
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
                                *timings.entry(rule.clone()).or_insert(Duration::new(0, 0)) +=
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
                                                .or_insert(Duration::new(0, 0)) += start.elapsed();
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
    let global_rule_names_for_filter =
        get_rule_names_by_isolation(host, enabled_rules, IsolationLevel::Global);
    let global_rule_ids: HashSet<&str> = global_rule_names_for_filter
        .iter()
        .map(|s| s.as_str())
        .collect();

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

    let tokens = tokenizer
        .tokenize(content)
        .map_err(|e| LinterError::Internal(format!("Tokenizer error: {}", e)))?;
    let sentences = SentenceSplitter::split(content, &ignore_ranges);

    let diagnostics =
        host.run_all_rules_with_parts(&ast_raw, content, &tokens, &sentences, path.to_str())?;

    Ok(diagnostics)
}

fn select_parser(extension: &str) -> Box<dyn Parser> {
    let md_parser = MarkdownParser::new();

    if md_parser.can_parse(extension) {
        Box::new(md_parser)
    } else {
        Box::new(PlainTextParser::new())
    }
}

fn to_raw_value<T: serde::Serialize>(
    value: &T,
    label: &str,
) -> Result<Box<serde_json::value::RawValue>, LinterError> {
    serde_json::value::to_raw_value(value)
        .map_err(|e| LinterError::Internal(format!("Failed to serialize {}: {}", label, e)))
}

fn get_rule_names_by_isolation(
    host: &PluginHost,
    enabled_rules: &HashSet<&str>,
    level: IsolationLevel,
) -> Vec<String> {
    let mut names = Vec::new();

    for name in host.loaded_rules() {
        if enabled_rules.contains(name.as_str())
            && let Some(manifest) = host.get_manifest(name)
            && manifest.isolation_level == level
        {
            names.push(name.clone());
        }
    }
    names
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
}
