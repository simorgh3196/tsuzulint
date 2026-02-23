//! Output formatting module

mod json;
mod sarif;
mod text;

use miette::Result;
use tsuzulint_core::LintResult;

use crate::cli::OutputFormat;

pub fn output_results(results: &[LintResult], format: OutputFormat, timings: bool) -> Result<bool> {
    let has_errors = results.iter().any(|r| r.has_errors());

    match format {
        OutputFormat::Sarif => sarif::output_sarif(results)?,
        OutputFormat::Json => json::output_json(results)?,
        OutputFormat::Text => text::output_text(results, timings),
    }

    Ok(has_errors)
}
