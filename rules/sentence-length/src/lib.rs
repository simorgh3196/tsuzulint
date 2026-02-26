//! sentence-length rule: Check sentence length.
//!
//! This rule reports sentences that exceed a configurable maximum length,
//! helping to improve readability.
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | max | number | 100 | Maximum sentence length in characters |
//! | skip_code | boolean | true | Skip code blocks and inline code |
//!
//! # Example
//!
//! ```json
//! {
//!   "rules": {
//!     "sentence-length": {
//!       "max": 80
//!     }
//!   }
//! }
//! ```

use extism_pdk::*;
use serde::Deserialize;
use tsuzulint_rule_pdk::{
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span, extract_node_text, get_sentences,
    is_node_type,
};

const RULE_ID: &str = "sentence-length";
const VERSION: &str = "1.0.0";
const DEFAULT_MAX_LENGTH: usize = 100;

/// Configuration for the sentence-length rule.
#[derive(Debug, Deserialize)]
struct Config {
    /// Maximum sentence length in characters.
    #[serde(default = "default_max")]
    max: usize,
    /// Skip code blocks and inline code.
    #[serde(default = "default_true", rename = "skip_code", alias = "_skip_code")]
    skip_code: bool,
}

fn default_max() -> usize {
    DEFAULT_MAX_LENGTH
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max: DEFAULT_MAX_LENGTH,
            skip_code: true,
        }
    }
}

/// Returns the rule manifest.
#[plugin_fn]
pub fn get_manifest() -> FnResult<Vec<u8>> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Check sentence length")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(rmp_serde::to_vec_named(&manifest)?)
}

/// Lints a node for sentence length.
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

    // Extract text from node
    if let Some((start, _end, text)) = extract_node_text(&request.node, &request.source) {
        // Split into sentences using PDK helper
        let sentences = get_sentences(text);

        for sentence in sentences {
            if sentence.char_count > config.max {
                diagnostics.push(Diagnostic::warning(
                    RULE_ID,
                    format!(
                        "Sentence is too long ({} characters). Maximum allowed is {}.",
                        sentence.char_count, config.max
                    ),
                    Span::new(
                        (start + sentence.start) as u32,
                        (start + sentence.end) as u32,
                    ),
                ));
            }
        }
    }

    Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tsuzulint_rule_pdk::AstNode;

    #[test]
    fn test_lint_simple() {
        let text = "Short sentence. Very long sentence that exceeds the limit definitely.";
        tsuzulint_rule_pdk::set_mock_config(serde_json::json!({ "max": 20 }));

        let request = LintRequest::single(
            AstNode::new("Str", Some([0, text.len() as u32])),
            text.to_string(),
        );

        let output = lint_impl(rmp_serde::to_vec_named(&request).unwrap()).unwrap();
        let response: LintResponse = rmp_serde::from_slice(&output).unwrap();

        // "Short sentence." is 15 chars (fine)
        // "Very long sentence..." is > 20 chars (warning)
        assert_eq!(response.diagnostics.len(), 1);
        assert!(
            response.diagnostics[0]
                .message
                .contains("Sentence is too long")
        );
    }

    #[test]
    fn test_lint_alias_config() {
        // Test compatibility with old _skip_code key
        // Verify that both `_skip_code` (legacy alias) and `skip_code` keys
        // are correctly deserialized into the Config struct.

        let config_json = r#"{ "max": 100, "_skip_code": false }"#;
        let config: Config = serde_json::from_str(config_json).unwrap();
        assert!(
            !config.skip_code,
            "legacy _skip_code key should be deserialized"
        );

        let config_json_new = r#"{ "max": 100, "skip_code": false }"#;
        let config_new: Config = serde_json::from_str(config_json_new).unwrap();
        assert!(
            !config_new.skip_code,
            "skip_code key should be deserialized"
        );
    }
}
