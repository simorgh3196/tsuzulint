//! JSON output formatter

use miette::{IntoDiagnostic, Result};
use tsuzulint_core::LintResult;

pub fn output_json(results: &[LintResult]) -> Result<()> {
    output_json_to(results, std::io::stdout())
}

pub(crate) fn output_json_to<W: std::io::Write>(
    results: &[LintResult],
    mut writer: W,
) -> Result<()> {
    let output: Vec<_> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "path": r.path.display().to_string(),
                "diagnostics": r.diagnostics,
            })
        })
        .collect();
    serde_json::to_writer_pretty(&mut writer, &output).into_diagnostic()?;
    writeln!(writer).into_diagnostic()?;
    Ok(())
}

#[cfg(test)]
mod tests {
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

        let mut buf = Vec::new();
        super::output_json_to(&[result], &mut buf).unwrap();

        let json_str = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let diag = &parsed[0]["diagnostics"][0];
        assert_eq!(diag["certainty"], "heuristic");
        assert_eq!(diag["metadata"]["foo"], "bar");
    }
}
