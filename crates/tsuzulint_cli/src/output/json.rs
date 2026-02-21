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
