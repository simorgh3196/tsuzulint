//! SARIF output formatter

use miette::{IntoDiagnostic, Result};
use tsuzulint_core::LintResult;

pub fn output_sarif(results: &[LintResult]) -> Result<()> {
    let stdout = std::io::stdout();
    let locked = stdout.lock();
    let buf_writer = std::io::BufWriter::new(locked);
    output_sarif_to(results, buf_writer)
}

pub(crate) fn output_sarif_to<W: std::io::Write>(
    results: &[LintResult],
    mut writer: W,
) -> Result<()> {
    tsuzulint_core::generate_sarif_to(results, &mut writer).into_diagnostic()?;
    writeln!(writer).into_diagnostic()?;
    writer.flush().into_diagnostic()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tsuzulint_core::{Diagnostic, LintResult};

    #[test]
    fn test_sarif_output_to() {
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
        super::output_sarif_to(&[result], &mut buf).unwrap();

        let json_str = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // Verify valid SARIF JSON
        assert_eq!(parsed["version"], "2.1.0");
        let run = &parsed["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "tsuzulint");
    }
}
