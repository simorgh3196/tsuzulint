//! SARIF output formatter

use miette::{IntoDiagnostic, Result};
use tsuzulint_core::LintResult;

pub fn output_sarif(results: &[LintResult]) -> Result<()> {
    let sarif_output = tsuzulint_core::generate_sarif(results).into_diagnostic()?;
    println!("{}", sarif_output);
    Ok(())
}
