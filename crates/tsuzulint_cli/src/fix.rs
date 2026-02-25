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
    pub errors: Vec<(PathBuf, String)>,
}

/// Applies fixes to all files with fixable diagnostics.
pub fn apply_fixes(results: &[LintResult], dry_run: bool) -> Result<FixSummary> {
    let mut total_fixes = 0;
    let mut files_fixed = 0;
    let mut fixes_by_file = Vec::new();
    let mut errors = Vec::new();

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
                    errors.push((result.path.clone(), e.to_string()));
                }
            }
        }
    }

    Ok(FixSummary {
        total_fixes,
        files_fixed,
        fixes_by_file,
        errors,
    })
}

/// Outputs the fix summary.
pub fn output_fix_summary(summary: &FixSummary, dry_run: bool) {
    if summary.total_fixes == 0 && summary.errors.is_empty() {
        println!("No fixable issues found.");
        return;
    }

    if summary.total_fixes > 0 {
        let action = if dry_run { "Would fix" } else { "Fixed" };
        let qualifier = if dry_run { "approximately " } else { "" };

        println!(
            "\n{} {}{} issues in {} files:",
            action, qualifier, summary.total_fixes, summary.files_fixed
        );
        print_fix_list(&summary.fixes_by_file);

        if dry_run {
            println!("\nRun without --dry-run to apply fixes.");
        }
    }

    if !summary.errors.is_empty() {
        eprintln!("\nFailed to fix {} file(s):", summary.errors.len());
        for (path, err) in &summary.errors {
            eprintln!("  {}: {}", path.display(), err);
        }
    }
}

fn print_fix_list(fixes_by_file: &[(PathBuf, usize)]) {
    for (path, count) in fixes_by_file {
        println!("  {}: {} fixes", path.display(), count);
    }
}
