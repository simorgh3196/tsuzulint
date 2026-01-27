//! no-todo rule: Disallow TODO/FIXME comments in text.
//!
//! This rule detects common task markers like TODO, FIXME, and XXX
//! that should be resolved before committing.
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | patterns | string[] | ["TODO:", "TODO ", "FIXME:", "FIXME ", "XXX:", "XXX "] | Patterns to detect |
//! | ignore_patterns | string[] | [] | Patterns to ignore |
//! | case_sensitive | boolean | false | Case-sensitive matching |
//!
//! # Example
//!
//! ```json
//! {
//!   "rules": {
//!     "no-todo": {
//!       "patterns": ["TODO:", "HACK:"],
//!       "case_sensitive": true
//!     }
//!   }
//! }
//! ```

use extism_pdk::*;
use serde::Deserialize;
use texide_rule_foundation::{
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span, extract_node_text, is_node_type,
};

const RULE_ID: &str = "no-todo";
const VERSION: &str = "1.0.0";

/// Default patterns to detect.
const DEFAULT_PATTERNS: &[&str] = &["TODO:", "TODO ", "FIXME:", "FIXME ", "XXX:", "XXX "];

/// Configuration for the no-todo rule.
#[derive(Debug, Deserialize, Default)]
struct Config {
    /// Patterns to detect (default: TODO:, FIXME:, XXX:).
    #[serde(default)]
    patterns: Vec<String>,
    /// Patterns to ignore.
    #[serde(default)]
    ignore_patterns: Vec<String>,
    /// Case-sensitive matching (default: false).
    #[serde(default)]
    case_sensitive: bool,
}

impl Config {
    /// Returns the patterns to check, using defaults if none specified.
    fn effective_patterns(&self) -> Vec<String> {
        if self.patterns.is_empty() {
            DEFAULT_PATTERNS.iter().map(|s| (*s).to_string()).collect()
        } else {
            self.patterns.clone()
        }
    }

    /// Checks if the given text should be ignored.
    fn should_ignore(&self, text: &str) -> bool {
        self.ignore_patterns.iter().any(|p| text.contains(p))
    }
}

/// Returns the rule manifest.
#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Disallow TODO/FIXME comments in text")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(serde_json::to_string(&manifest)?)
}

/// Finds all occurrences of a pattern in text.
fn find_pattern_matches(
    text: &str,
    pattern: &str,
    case_sensitive: bool,
    base_offset: usize,
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();

    let (search_text, search_pattern) = if case_sensitive {
        (text.to_string(), pattern.to_string())
    } else {
        (text.to_uppercase(), pattern.to_uppercase())
    };

    let mut search_start = 0;
    while let Some(pos) = search_text[search_start..].find(&search_pattern) {
        let abs_pos = search_start + pos;
        let match_start = base_offset + abs_pos;
        let match_end = match_start + pattern.len();
        matches.push((match_start, match_end));
        search_start = abs_pos + pattern.len();
    }

    matches
}

/// Lints a node for TODO patterns.
#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Only process Str nodes
    if !is_node_type(&request.node, "Str") {
        return Ok(serde_json::to_string(&LintResponse { diagnostics })?);
    }

    // Parse configuration
    let config: Config = serde_json::from_value(request.config.clone()).unwrap_or_default();

    // Get patterns to check
    let patterns = config.effective_patterns();

    // Extract text from node
    if let Some((start, _end, text)) = extract_node_text(&request.node, &request.source) {
        for pattern in &patterns {
            let matches = find_pattern_matches(text, pattern, config.case_sensitive, start);

            for (match_start, match_end) in matches {
                // Get the original text at this position for ignore check
                let original_pos = match_start - start;
                let original_text = &text[original_pos..original_pos + pattern.len()];

                if config.should_ignore(original_text) {
                    continue;
                }

                diagnostics.push(Diagnostic::warning(
                    RULE_ID,
                    format!(
                        "Found '{}' comment. Consider resolving this before committing.",
                        pattern.trim()
                    ),
                    Span::new(match_start as u32, match_end as u32),
                ));
            }
        }
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn create_request(text: &str, config: serde_json::Value) -> String {
        let node = serde_json::json!({
            "type": "Str",
            "range": [0, text.len()]
        });
        let request = serde_json::json!({
            "node": node,
            "config": config,
            "source": text,
            "file_path": null
        });
        serde_json::to_string(&request).unwrap()
    }

    fn parse_response(json: &str) -> LintResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn config_default_patterns() {
        let config = Config::default();
        let patterns = config.effective_patterns();
        assert!(patterns.contains(&"TODO:".to_string()));
        assert!(patterns.contains(&"FIXME:".to_string()));
        assert!(patterns.contains(&"XXX:".to_string()));
    }

    #[test]
    fn config_custom_patterns() {
        let config = Config {
            patterns: vec!["HACK:".to_string()],
            ..Default::default()
        };
        let patterns = config.effective_patterns();
        assert_eq!(patterns.len(), 1);
        assert!(patterns.contains(&"HACK:".to_string()));
    }

    #[test]
    fn config_should_ignore() {
        let config = Config {
            ignore_patterns: vec!["TODO-OK".to_string()],
            ..Default::default()
        };
        assert!(config.should_ignore("TODO-OK: this is fine"));
        assert!(!config.should_ignore("TODO: this should be flagged"));
    }

    #[test]
    fn find_pattern_matches_case_insensitive() {
        // "todo: fix this TODO: and that"
        //  0    5    10   15   20   25
        // "todo:" at 0-5, "TODO:" at 15-20
        let matches = find_pattern_matches("todo: fix this TODO: and that", "TODO:", false, 0);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], (0, 5));
        assert_eq!(matches[1], (15, 20));
    }

    #[test]
    fn find_pattern_matches_case_sensitive() {
        // "todo: fix this TODO: and that"
        //  0    5    10   15   20   25
        // Only "TODO:" at position 15-20 matches (case sensitive)
        let matches = find_pattern_matches("todo: fix this TODO: and that", "TODO:", true, 0);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], (15, 20));
    }

    #[test]
    fn find_pattern_matches_with_offset() {
        let matches = find_pattern_matches("TODO: test", "TODO:", true, 100);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], (100, 105));
    }

    #[test]
    fn manifest_contains_required_fields() {
        // Test manifest structure directly (plugin_fn macro changes signature at compile time)
        let manifest = RuleManifest::new(RULE_ID, VERSION)
            .with_description("Disallow TODO/FIXME comments in text")
            .with_fixable(false)
            .with_node_types(vec!["Str".to_string()]);

        assert_eq!(manifest.name, RULE_ID);
        assert_eq!(manifest.version, VERSION);
        assert!(manifest.description.is_some());
        assert!(!manifest.fixable);
        assert!(manifest.node_types.contains(&"Str".to_string()));

        // Verify it serializes correctly
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(RULE_ID));
    }
}
