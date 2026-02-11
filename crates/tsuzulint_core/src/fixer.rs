//! Auto-fix functionality for applying diagnostic fixes.

use std::fs;
use std::path::Path;

use tracing::{debug, warn};

use tsuzulint_plugin::{Diagnostic, Fix};

use crate::LinterError;

/// Result of applying fixes to a file.
#[derive(Debug)]
pub struct FixerResult {
    /// Number of fixes applied.
    pub fixes_applied: usize,
    /// The fixed content.
    pub fixed_content: String,
    /// Whether the content was modified.
    pub modified: bool,
}

impl FixerResult {
    /// Creates a new fixer result.
    pub fn new(fixes_applied: usize, fixed_content: String, modified: bool) -> Self {
        Self {
            fixes_applied,
            fixed_content,
            modified,
        }
    }

    /// Creates a result indicating no changes were made.
    pub fn unchanged(content: String) -> Self {
        Self {
            fixes_applied: 0,
            fixed_content: content,
            modified: false,
        }
    }
}

/// Applies fixes to content.
///
/// Fixes are applied from the end of the file to the beginning to avoid
/// offset issues when multiple fixes are applied.
pub fn apply_fixes_to_content(content: &str, diagnostics: &[Diagnostic]) -> FixerResult {
    // Extract fixes from diagnostics
    let fixes: Vec<&Fix> = diagnostics.iter().filter_map(|d| d.fix.as_ref()).collect();

    if fixes.is_empty() {
        return FixerResult::unchanged(content.to_string());
    }

    // Sort by span.start in descending order (apply from end to beginning)
    let mut sorted_fixes: Vec<&Fix> = fixes;
    sorted_fixes.sort_by(|a, b| b.span.start.cmp(&a.span.start));

    // Check for overlapping spans
    let sorted_fixes = filter_overlapping_fixes(sorted_fixes);

    // Apply fixes
    let mut result = content.to_string();
    let mut applied = 0;

    for fix in &sorted_fixes {
        let start = fix.span.start as usize;
        let end = fix.span.end as usize;

        // Validate bounds
        if start > result.len() || end > result.len() || start > end {
            warn!(
                "Invalid fix span: start={}, end={}, content_len={}",
                start,
                end,
                result.len()
            );
            continue;
        }

        debug!(
            "Applying fix: replace [{}..{}] with '{}'",
            start, end, fix.text
        );

        result.replace_range(start..end, &fix.text);
        applied += 1;
    }

    FixerResult::new(applied, result, applied > 0)
}

/// Filters out overlapping fixes, keeping the one that starts later.
///
/// **Note:** This function expects `fixes` to be sorted by `start` position in descending order.
/// If they are not sorted, the filtering logic will be incorrect.
pub(crate) fn filter_overlapping_fixes(fixes: Vec<&Fix>) -> Vec<&Fix> {
    if fixes.len() <= 1 {
        return fixes;
    }

    // Verify sorted assumption in debug builds
    #[cfg(debug_assertions)]
    {
        for window in fixes.windows(2) {
            debug_assert!(
                window[0].span.start >= window[1].span.start,
                "Fixes must be sorted by start descending for filter_overlapping_fixes"
            );
        }
    }

    let mut result: Vec<&Fix> = Vec::with_capacity(fixes.len());

    for fix in fixes {
        // Since fixes are sorted by start descending, and we process them in that order:
        // - `result` will also be sorted by start descending.
        // - `result.last()` is the fix with the smallest start position among those already accepted.
        // - `fix` (current candidate) has a start position <= any fix in `result`.
        //
        // Therefore, if `fix` overlaps with *any* accepted fix, it must overlap with `result.last()`.
        // We only need to check overlap against the last accepted fix.
        let overlaps = if let Some(last) = result.last() {
            // Let's stick to the full check for safety and clarity, but optimized to only check one item.
            let existing_start = last.span.start;
            let existing_end = last.span.end;
            let fix_start = fix.span.start;
            let fix_end = fix.span.end;

            !(fix_end <= existing_start || fix_start >= existing_end)
        } else {
            false
        };

        if overlaps {
            warn!(
                "Skipping overlapping fix at [{}, {}]",
                fix.span.start, fix.span.end
            );
        } else {
            result.push(fix);
        }
    }

    result
}

/// Applies fixes to a file and writes the result.
pub fn apply_fixes_to_file(
    path: &Path,
    diagnostics: &[Diagnostic],
) -> Result<FixerResult, LinterError> {
    let content = fs::read_to_string(path)
        .map_err(|e| LinterError::file(format!("Failed to read {}: {}", path.display(), e)))?;

    let result = apply_fixes_to_content(&content, diagnostics);

    if result.modified {
        fs::write(path, &result.fixed_content)
            .map_err(|e| LinterError::file(format!("Failed to write {}: {}", path.display(), e)))?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::Span;

    fn make_diagnostic_with_fix(start: u32, end: u32, replacement: &str) -> Diagnostic {
        Diagnostic::new("test-rule", "Test message", Span::new(start, end))
            .with_fix(Fix::new(Span::new(start, end), replacement))
    }

    fn make_diagnostic_without_fix(start: u32, end: u32) -> Diagnostic {
        Diagnostic::new("test-rule", "Test message", Span::new(start, end))
    }

    #[test]
    fn apply_single_fix() {
        let content = "Hello World";
        let diagnostics = vec![make_diagnostic_with_fix(0, 5, "Hi")];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "Hi World");
        assert_eq!(result.fixes_applied, 1);
        assert!(result.modified);
    }

    #[test]
    fn apply_multiple_fixes() {
        let content = "Hello World";
        let diagnostics = vec![
            make_diagnostic_with_fix(0, 5, "Hi"),
            make_diagnostic_with_fix(6, 11, "Earth"),
        ];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "Hi Earth");
        assert_eq!(result.fixes_applied, 2);
        assert!(result.modified);
    }

    #[test]
    fn apply_delete_fix() {
        let content = "Hello World";
        // Delete " World"
        let diagnostics = vec![make_diagnostic_with_fix(5, 11, "")];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "Hello");
        assert_eq!(result.fixes_applied, 1);
    }

    #[test]
    fn apply_insert_fix() {
        let content = "HelloWorld";
        // Insert " " at position 5
        let diagnostics = vec![make_diagnostic_with_fix(5, 5, " ")];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "Hello World");
        assert_eq!(result.fixes_applied, 1);
    }

    #[test]
    fn no_fixes_returns_unchanged() {
        let content = "Hello World";
        let diagnostics: Vec<Diagnostic> = vec![];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "Hello World");
        assert_eq!(result.fixes_applied, 0);
        assert!(!result.modified);
    }

    #[test]
    fn diagnostics_without_fix_are_skipped() {
        let content = "Hello World";
        let diagnostics = vec![
            make_diagnostic_without_fix(0, 5),
            make_diagnostic_with_fix(6, 11, "Earth"),
        ];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "Hello Earth");
        assert_eq!(result.fixes_applied, 1);
    }

    #[test]
    fn overlapping_fixes_are_filtered() {
        let content = "Hello World";
        // Two fixes that overlap: [0, 5] and [3, 8]
        let diagnostics = vec![
            make_diagnostic_with_fix(0, 5, "Hi"),
            make_diagnostic_with_fix(3, 8, "XXX"), // overlaps with first, should be skipped
        ];

        let result = apply_fixes_to_content(content, &diagnostics);

        // Only the first fix (which starts later when sorted descending) should apply
        // Actually [3, 8] starts later, so it takes precedence
        assert_eq!(result.fixes_applied, 1);
    }

    #[test]
    fn japanese_text_fix() {
        let content = "東京にに行く";
        // "に" at byte positions: "東京" = 6 bytes, first "に" = [6, 9], second "に" = [9, 12]
        // Fix: delete second "に"
        let diagnostics = vec![make_diagnostic_with_fix(9, 12, "")];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "東京に行く");
        assert_eq!(result.fixes_applied, 1);
    }

    #[test]
    fn multiple_japanese_fixes() {
        let content = "私ははは学生";
        // "私" = 3 bytes, each "は" = 3 bytes
        // positions: 私[0-3], は[3-6], は[6-9], は[9-12], 学[12-15], 生[15-18]
        // Fix: delete third "は" at [9, 12]
        let diagnostics = vec![make_diagnostic_with_fix(9, 12, "")];

        let result = apply_fixes_to_content(content, &diagnostics);

        assert_eq!(result.fixed_content, "私はは学生");
        assert_eq!(result.fixes_applied, 1);
    }

    #[test]
    fn invalid_span_is_skipped() {
        let content = "Hello";
        // Invalid span: end > content length
        let diagnostics = vec![make_diagnostic_with_fix(0, 100, "Hi")];

        let result = apply_fixes_to_content(content, &diagnostics);

        // Fix should be skipped, content unchanged
        assert_eq!(result.fixed_content, "Hello");
        assert_eq!(result.fixes_applied, 0);
    }

    #[test]
    fn fixer_result_unchanged() {
        let result = FixerResult::unchanged("test".to_string());
        assert_eq!(result.fixes_applied, 0);
        assert!(!result.modified);
        assert_eq!(result.fixed_content, "test");
    }

    #[test]
    fn filter_overlapping_fixes_basic() {
        let f1 = Fix::new(Span::new(10, 15), "f1");
        let f2 = Fix::new(Span::new(0, 5), "f2");
        let fixes = vec![&f1, &f2]; // Sorted descending: 10, 0

        let result = filter_overlapping_fixes(fixes);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "f1");
        assert_eq!(result[1].text, "f2");
    }

    #[test]
    fn filter_overlapping_fixes_overlap() {
        let f1 = Fix::new(Span::new(10, 20), "f1");
        let f2 = Fix::new(Span::new(15, 25), "f2"); // Overlaps f1
        let f3 = Fix::new(Span::new(0, 5), "f3");

        // Sorted descending: f2 (15), f1 (10), f3 (0)
        let fixes = vec![&f2, &f1, &f3];

        let result = filter_overlapping_fixes(fixes);

        // f2 is kept. f1 overlaps f2?
        // f1: [10, 20). f2: [15, 25).
        // f1 start (10) < f2 end (25).
        // f1 end (20) > f2 start (15).
        // Overlap! f1 should be skipped.

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "f2");
        assert_eq!(result[1].text, "f3");
    }

    #[test]
    fn filter_overlapping_fixes_adjacent() {
        let f1 = Fix::new(Span::new(10, 15), "f1");
        let f2 = Fix::new(Span::new(5, 10), "f2"); // Ends exactly where f1 starts

        // Sorted descending: f1 (10), f2 (5)
        let fixes = vec![&f1, &f2];

        let result = filter_overlapping_fixes(fixes);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "f1");
        assert_eq!(result[1].text, "f2");
    }

    #[test]
    fn filter_overlapping_fixes_nested() {
        let f1 = Fix::new(Span::new(0, 20), "outer");
        let f2 = Fix::new(Span::new(5, 15), "inner");

        // Sorted descending: f2 (5), f1 (0) -> No. f1 is 0. f2 is 5.
        // Wait, f2 start is 5. f1 start is 0.
        // Sorted descending: f2 (5), f1 (0).
        let fixes = vec![&f2, &f1];

        let result = filter_overlapping_fixes(fixes);

        // f2 is kept.
        // Check f1 vs f2.
        // f1: [0, 20). f2: [5, 15).
        // f1 end (20) > f2 start (5).
        // f1 start (0) < f2 end (15).
        // Overlap! f1 skipped.

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "inner");
    }

    #[test]
    fn filter_overlapping_fixes_zero_width_same_pos() {
        // Two insertions at same position
        let f1 = Fix::insert(10, "A");
        let f2 = Fix::insert(10, "B");

        // Sorted descending: both 10. Order preserved (f1 then f2 if stable sort, but here we construct input manually)
        let fixes = vec![&f1, &f2];

        let result = filter_overlapping_fixes(fixes);

        // f1 kept.
        // Check f2 vs f1.
        // f2: [10, 10). f1: [10, 10).
        // f2 end (10) <= f1 start (10)? True.
        // No overlap.

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_overlapping_fixes_zero_width_inside() {
        let f1 = Fix::new(Span::new(0, 10), "replace");
        let f2 = Fix::insert(5, "insert");

        // Sorted descending: f2 (5), f1 (0).
        let fixes = vec![&f2, &f1];

        let result = filter_overlapping_fixes(fixes);

        // f2 kept.
        // f1 vs f2.
        // f1: [0, 10). f2: [5, 5).
        // f1 end (10) > f2 start (5).
        // f1 start (0) < f2 end (5).
        // Overlap! f1 skipped.

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "insert");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Fixes must be sorted")]
    fn filter_overlapping_fixes_panic_unsorted() {
        let f1 = Fix::new(Span::new(0, 5), "f1");
        let f2 = Fix::new(Span::new(10, 15), "f2");

        // Unsorted (0 then 10)
        let fixes = vec![&f1, &f2];

        let _ = filter_overlapping_fixes(fixes);
    }
}
