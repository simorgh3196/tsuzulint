use extism_pdk::*;
use serde_json::json;
use tsuzulint_rule_pdk::{Diagnostic, LintRequest, LintResponse, RuleManifest, Span};

const RULE_ID: &str = "test-rule";
const VERSION: &str = "1.0.0";

#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("A simple test rule")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Check if the source contains "error"
    if request.source.contains("error") {
        for (idx, _) in request.source.match_indices("error") {
             diagnostics.push(Diagnostic::new(
                RULE_ID,
                "Found error keyword",
                Span::new(idx as u32, (idx + 5) as u32),
            ));
        }
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}
