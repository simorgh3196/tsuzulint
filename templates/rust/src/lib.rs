//! {{RULE_NAME}} rule: {{RULE_DESCRIPTION}}
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | example_option | string | "default" | Example configuration option |
//!
//! # Example
//!
//! ```json
//! {
//!   "rules": {
//!     "{{RULE_NAME}}": {
//!       "example_option": "custom_value"
//!     }
//!   }
//! }
//! ```

use extism_pdk::*;
use serde::Deserialize;
use texide_rule_foundation::{
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span, extract_node_text, is_node_type,
};

// Rule metadata
const RULE_ID: &str = "{{RULE_NAME}}";
const VERSION: &str = "0.1.0";

/// Configuration for the {{RULE_NAME}} rule.
#[derive(Debug, Deserialize, Default)]
struct Config {
    /// Example configuration option.
    #[serde(default = "default_example_option")]
    example_option: String,
}

fn default_example_option() -> String {
    "default".to_string()
}

/// Returns the rule manifest.
#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("{{RULE_DESCRIPTION}}")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(serde_json::to_string(&manifest)?)
}

/// Lints a node for rule violations.
#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Only process Str nodes (modify node_types in manifest if needed)
    if !is_node_type(&request.node, "Str") {
        return Ok(serde_json::to_string(&LintResponse { diagnostics })?);
    }

    // Parse configuration
    let _config: Config = serde_json::from_value(request.config.clone()).unwrap_or_default();

    // Extract text from node
    if let Some((start, end, text)) = extract_node_text(&request.node, &request.source) {
        // TODO: Implement your lint logic here
        //
        // Example: Check for a specific pattern
        // if text.contains("BAD_PATTERN") {
        //     diagnostics.push(Diagnostic::warning(
        //         RULE_ID,
        //         "Found bad pattern in text",
        //         Span::new(start as u32, end as u32),
        //     ));
        // }

        // Placeholder: remove this once you implement your logic
        let _ = (start, end, text);
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Helper to create a lint request JSON.
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

    /// Helper to parse a lint response JSON.
    fn parse_response(json: &str) -> LintResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn manifest_contains_required_fields() {
        let manifest = RuleManifest::new(RULE_ID, VERSION)
            .with_description("{{RULE_DESCRIPTION}}")
            .with_fixable(false)
            .with_node_types(vec!["Str".to_string()]);

        assert_eq!(manifest.name, RULE_ID);
        assert_eq!(manifest.version, VERSION);
        assert!(manifest.description.is_some());
    }

    #[test]
    fn config_default_values() {
        let config: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(config.example_option, "default");
    }

    #[test]
    fn lint_clean_text_returns_no_diagnostics() {
        let input = create_request("This is clean text", serde_json::json!({}));
        // Note: In actual tests, you'd call the function directly without #[plugin_fn]
        // For now, test the internal logic
        let request: LintRequest = serde_json::from_str(&input).unwrap();
        let diagnostics: Vec<Diagnostic> = Vec::new();

        // Your test assertions here
        assert!(diagnostics.is_empty());
        assert!(is_node_type(&request.node, "Str"));
    }

    // TODO: Add more tests for your specific rule logic
    // #[test]
    // fn lint_detects_violation() {
    //     let input = create_request("Text with violation", serde_json::json!({}));
    //     // ...
    // }
}
