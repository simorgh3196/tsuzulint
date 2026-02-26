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
use tsuzulint_rule_pdk::{
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span, extract_node_text, find_matches,
    is_node_type,
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
pub fn get_manifest() -> FnResult<Vec<u8>> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Disallow TODO/FIXME comments in text")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(rmp_serde::to_vec_named(&manifest)?)
}

/// Lints a node for TODO patterns.
#[plugin_fn]
pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>> {
    lint_impl(input)
}

fn lint_impl(input: Vec<u8>) -> FnResult<Vec<u8>> {
    let request: LintRequest = rmp_serde::from_slice(&input)?;
    let mut diagnostics = Vec::new();

    // Only process Str nodes
    if !is_node_type(&request.node, "Str") {
        return Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?);
    }

    // Parse configuration
    let config: Config = tsuzulint_rule_pdk::get_config().unwrap_or_default();

    // Get patterns to check
    let patterns = config.effective_patterns();

    // Extract text from node
    if let Some((start, _end, text)) = extract_node_text(&request.node, &request.source) {
        // Use PDK helper to find matches
        let matches = find_matches(text, &patterns, config.case_sensitive);

        for m in matches {
            // Check if we should ignore this match (using the exact matched text)
            // Note: find_matches returns the original case-preserved text in matched_text
            if config.should_ignore(&m.matched_text) {
                continue;
            }

            // Calculate absolute positions
            let match_start = start + m.start;
            let match_end = start + m.end;

            diagnostics.push(Diagnostic::warning(
                RULE_ID,
                format!(
                    "Found '{}' comment. Consider resolving this before committing.",
                    m.matched_text.trim()
                ),
                Span::new(match_start as u32, match_end as u32),
            ));
        }
    }

    Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tsuzulint_rule_pdk::AstNode;

    fn create_request(text: &str) -> Vec<u8> {
        let request = LintRequest::single(
            AstNode::new("Str", Some([0, text.len() as u32])),
            text.to_string(),
        );
        rmp_serde::to_vec_named(&request).unwrap()
    }

    fn parse_response(bytes: &[u8]) -> LintResponse {
        rmp_serde::from_slice(bytes).unwrap()
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
    fn lint_detects_todo() {
        let input = create_request("This is a TODO: check");
        let output = lint_impl(input).unwrap();
        let response = parse_response(&output);
        assert_eq!(response.diagnostics.len(), 1);
        assert_eq!(
            response.diagnostics[0].message,
            "Found 'TODO:' comment. Consider resolving this before committing."
        );
    }

    #[test]
    fn lint_ignores_pattern() {
        // "TODO: fix later" matches "TODO:", but ignore pattern "TODO:" filters it out.
        tsuzulint_rule_pdk::set_mock_config(&serde_json::json!({
            "ignore_patterns": ["TODO:"]
        }));
        let input = create_request("This is a TODO: fix later");
        let output = lint_impl(input).unwrap();
        let response = parse_response(&output);

        assert_eq!(response.diagnostics.len(), 0);
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

        // Verify it serializes correctly via MsgPack
        let bytes = rmp_serde::to_vec_named(&manifest).unwrap();
        let decoded: RuleManifest = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.name, RULE_ID);
    }
}
