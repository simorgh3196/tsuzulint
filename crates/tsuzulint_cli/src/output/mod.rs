//! Output formatting module

mod json;
mod sarif;
mod text;

use miette::Result;
use tsuzulint_core::LintResult;

pub fn output_results(results: &[LintResult], format: &str, timings: bool) -> Result<bool> {
    let has_errors = results.iter().any(|r| r.has_errors());

    match format {
        "sarif" => sarif::output_sarif(results)?,
        "json" => json::output_json(results)?,
        _ => text::output_text(results, timings),
    }

    Ok(has_errors)
}
