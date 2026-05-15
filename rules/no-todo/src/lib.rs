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
pub fn get_manifest() -> FnResult<RuleManifest> {
    Ok(RuleManifest::new(RULE_ID, VERSION)
        .with_description("Disallow TODO/FIXME comments in text")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]))
}

/// Lints a node for TODO patterns.
#[plugin_fn]
pub fn lint(request: LintRequest) -> FnResult<LintResponse> {
    let mut diagnostics = Vec::new();

    let config: Config = request.get_config().unwrap_or_default();
    let patterns = config.effective_patterns();

    // `all_nodes()` yields the batch when the host sent one, or a single
    // element wrapping `request.node` for the legacy single-node protocol.
    // Either way we only act on Str nodes; the host already filters by the
    // manifest's `node_types`, but staying defensive keeps the rule correct
    // if it is ever invoked with a wider node set.
    for node in request.all_nodes() {
        if !is_node_type(node, "Str") {
            continue;
        }

        let Some((start, _end, text)) = extract_node_text(node, &request.source) else {
            continue;
        };

        for m in find_matches(text, &patterns, config.case_sensitive) {
            if config.should_ignore(&m.matched_text) {
                continue;
            }

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

    Ok(LintResponse { diagnostics })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tsuzulint_rule_pdk::AstNode;

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

    #[cfg(target_arch = "wasm32")]
    fn create_request(text: &str) -> LintRequest {
        LintRequest::single(
            AstNode::new("Str", Some([0, text.len() as u32])),
            text.to_string(),
        )
    }

    #[cfg(target_arch = "wasm32")]
    fn create_request_with_config<T: serde::Serialize>(text: &str, config: &T) -> LintRequest {
        let mut request = LintRequest::single(
            AstNode::new("Str", Some([0, text.len() as u32])),
            text.to_string(),
        );
        request.config = Some(rmp_serde::to_vec_named(config).unwrap());
        request
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn lint_detects_todo() {
        let request = create_request("This is a TODO: check");
        let response = lint(request).unwrap();
        assert_eq!(response.diagnostics.len(), 1);
        assert_eq!(
            response.diagnostics[0].message,
            "Found 'TODO:' comment. Consider resolving this before committing."
        );
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn lint_ignores_pattern() {
        let request = create_request_with_config(
            "This is a TODO: fix later",
            &serde_json::json!({ "ignore_patterns": ["TODO:"] }),
        );
        let response = lint(request).unwrap();
        assert_eq!(response.diagnostics.len(), 0);
    }
}
