//! Fix application logic

use std::path::PathBuf;

use miette::Result;
use tracing::error;
use tsuzulint_core::{LintResult, apply_fixes_to_file};

/// Summary of applied fixes.
pub struct FixSummary {
    pub total_fixes: usize,
    pub files_fixed: usize,
    pub fixes_by_file: Vec<(PathBuf, usize)>,
}

/// Applies fixes to all files with fixable diagnostics.
pub fn apply_fixes(results: &[LintResult], dry_run: bool) -> Result<FixSummary> {
    let mut total_fixes = 0;
    let mut files_fixed = 0;
    let mut fixes_by_file = Vec::new();

    for result in results {
        let fixable_count = result
            .diagnostics
            .iter()
            .filter(|d| d.fix.is_some())
            .count();

        if fixable_count == 0 {
            continue;
        }

        if dry_run {
            fixes_by_file.push((result.path.clone(), fixable_count));
            total_fixes += fixable_count;
            files_fixed += 1;
        } else {
            match apply_fixes_to_file(&result.path, &result.diagnostics) {
                Ok(fixer_result) => {
                    if fixer_result.modified {
                        fixes_by_file.push((result.path.clone(), fixer_result.fixes_applied));
                        total_fixes += fixer_result.fixes_applied;
                        files_fixed += 1;
                    }
                }
                Err(e) => {
                    error!("Failed to fix {}: {}", result.path.display(), e);
                }
            }
        }
    }

    Ok(FixSummary {
        total_fixes,
        files_fixed,
        fixes_by_file,
    })
}

/// Outputs the fix summary.
pub fn output_fix_summary(summary: &FixSummary, dry_run: bool) {
    if summary.total_fixes == 0 {
        println!("No fixable issues found.");
        return;
    }

    if dry_run {
        println!(
            "\nWould fix {} issues in {} files:",
            summary.total_fixes, summary.files_fixed
        );
        for (path, count) in &summary.fixes_by_file {
            println!("  {}: {} fixes", path.display(), count);
        }
        println!("\nRun without --dry-run to apply fixes.");
    } else {
        println!(
            "\nFixed {} issues in {} files:",
            summary.total_fixes, summary.files_fixed
        );
        for (path, count) in &summary.fixes_by_file {
            println!("  {}: {} fixes", path.display(), count);
        }
    }
}
