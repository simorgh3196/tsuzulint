use extism_pdk::*;
use tsuzulint_rule_pdk::{Diagnostic, LintRequest, LintResponse, RuleManifest, Span};

const RULE_ID: &str = "test-rule";
const VERSION: &str = "1.0.0";

#[plugin_fn]
pub fn get_manifest() -> FnResult<RuleManifest> {
    Ok(RuleManifest::new(RULE_ID, VERSION)
        .with_description("A simple test rule")
        .with_fixable(false))
}

#[plugin_fn]
pub fn lint(request: LintRequest) -> FnResult<LintResponse> {
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

    Ok(LintResponse { diagnostics })
}
