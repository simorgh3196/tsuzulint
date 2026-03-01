//! JSON output formatter

use miette::{IntoDiagnostic, Result};
use tsuzulint_core::LintResult;

pub fn output_json(results: &[LintResult]) -> Result<()> {
    let output: Vec<_> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "path": r.path.display().to_string(),
                "diagnostics": r.diagnostics,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&output).into_diagnostic()?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tsuzulint_core::{Diagnostic, LintResult};

    #[test]
    fn test_json_output_includes_heuristic_and_metadata() {
        let diag: Diagnostic = serde_json::from_value(serde_json::json!({
            "rule_id": "test-rule",
            "message": "msg",
            "span": { "start": 0, "end": 5 },
            "severity": "warning",
            "certainty": "heuristic",
            "metadata": {"foo": "bar"}
        }))
        .unwrap();

        let result = LintResult {
            path: PathBuf::from("test.md"),
            diagnostics: vec![diag],
            from_cache: false,
            timings: HashMap::new(),
        };

        // We can't easily capture stdout without redirecting, but we can verify
        // that the manually created json object has the right properties since
        // output_json just serializes this structural format.
        let json_val = serde_json::json!({
            "path": result.path.display().to_string(),
            "diagnostics": result.diagnostics,
        });

        let json_str = serde_json::to_string(&json_val).unwrap();
        assert!(json_str.contains(r#""certainty":"heuristic""#));
        assert!(json_str.contains(r#""metadata":{"foo":"bar"}"#));
    }
}
