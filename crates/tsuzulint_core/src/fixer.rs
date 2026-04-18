//! Auto-fix functionality for applying diagnostic fixes.

use std::fs;
use std::path::Path;

use tracing::{debug, warn};

use tsuzulint_plugin::{Diagnostic, Fix};

use crate::LinterError;
use crate::file_linter::MAX_FILE_SIZE;
use crate::safe_io::{
    check_file_metadata, clear_nonblocking, handle_io_err, open_nonblocking, read_to_string_bounded,
};

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

    // Filter and sort fixes
    let sorted_fixes = filter_overlapping_fixes(fixes);

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
/// This function sorts the fixes by start position in descending order
/// before filtering to ensure consistent results.
pub(crate) fn filter_overlapping_fixes(mut fixes: Vec<&Fix>) -> Vec<&Fix> {
    if fixes.len() <= 1 {
        return fixes;
    }

    // Sort by span.start in descending order (apply from end to beginning)
    fixes.sort_by(|a, b| b.span.start.cmp(&a.span.start));

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

/// Reads `path` safely: rejects non-regular files (incl. FIFOs that would
/// otherwise block), caps the read at `max_size` bytes to prevent
/// memory-exhaustion via pseudo-files (e.g. `/dev/zero`), and uses
/// `O_NONBLOCK` on Unix to close the TOCTOU window around the open itself.
fn read_file_bounded(path: &Path, max_size: u64) -> Result<String, LinterError> {
    // O_NONBLOCK open: never blocks on FIFOs or TTYs, and gives us an fd we
    // can fstat (instead of racing a lstat/open pair).
    let mut file = handle_io_err(open_nonblocking(path), path, "Failed to open")?;

    // fstat-based metadata on the opened fd; no TOCTOU window vs. a separate
    // lstat on `path`.  This replaces the previous
    // `file.metadata().unwrap_or_else(|_| unreachable!())` which could panic
    // on platforms where `fstat` can still fail after a successful open.
    let metadata = handle_io_err(file.metadata(), path, "Failed to read metadata for")?;
    check_file_metadata(&metadata, max_size, path)?;

    // Clear O_NONBLOCK so the subsequent read can block as usual.
    handle_io_err(
        clear_nonblocking(&file),
        path,
        "Failed to clear O_NONBLOCK on",
    )?;

    // Bounded read: `/proc/version`, `/dev/zero`, and similar pseudo-files
    // may report `metadata.len() == 0` while producing arbitrarily many
    // bytes, so we must cap at the content level too.
    read_to_string_bounded(&mut file, max_size, path)
}

fn apply_fixes_to_file_inner(
    path: &Path,
    diagnostics: &[Diagnostic],
    max_size: u64,
) -> Result<FixerResult, LinterError> {
    let content = read_file_bounded(path, max_size)?;
    Ok(apply_fixes_to_content(&content, diagnostics))
}

/// Applies fixes to a file and writes the result.
pub fn apply_fixes_to_file(
    path: &Path,
    diagnostics: &[Diagnostic],
) -> Result<FixerResult, LinterError> {
    let result = apply_fixes_to_file_inner(path, diagnostics, MAX_FILE_SIZE)?;

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
    fn filter_overlapping_fixes_unsorted() {
        let f1 = Fix::new(Span::new(0, 5), "f1");
        let f2 = Fix::new(Span::new(10, 15), "f2");

        // Unsorted (0 then 10)
        let fixes = vec![&f1, &f2];

        let result = filter_overlapping_fixes(fixes);

        // Should be sorted to f2 (10), f1 (0) and kept
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "f2");
        assert_eq!(result[1].text, "f1");
    }

    #[test]
    fn apply_fixes_to_file_success() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary file with content
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        write!(file, "Hello World").expect("Failed to write to temp file");

        // Close the file handle but keep the path valid for the test duration
        // This is important for Windows where open file handles prevent other access
        let path = file.into_temp_path();

        // Create diagnostic with fix: replace "World" [6..11] with "Rust"
        let diagnostics = vec![make_diagnostic_with_fix(6, 11, "Rust")];

        // Apply fixes
        let result = apply_fixes_to_file(&path, &diagnostics).expect("apply_fixes_to_file failed");

        // Verify result
        assert!(result.modified);
        assert_eq!(result.fixes_applied, 1);
        assert_eq!(result.fixed_content, "Hello Rust");

        // Verify file content on disk changed
        let content = fs::read_to_string(&path).expect("Failed to read file back");
        assert_eq!(content, "Hello Rust");
    }

    #[test]
    fn apply_fixes_to_file_no_changes() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        write!(file, "Hello World").expect("Failed to write to temp file");

        // Close the file handle but keep the path valid
        let path = file.into_temp_path();

        // No diagnostics
        let diagnostics: Vec<Diagnostic> = vec![];

        let result = apply_fixes_to_file(&path, &diagnostics).expect("apply_fixes_to_file failed");

        // Verify result
        assert!(!result.modified);
        assert_eq!(result.fixes_applied, 0);
        assert_eq!(result.fixed_content, "Hello World");

        // Verify file content on disk unchanged
        let content = fs::read_to_string(&path).expect("Failed to read file back");
        assert_eq!(content, "Hello World");
    }

    #[test]
    fn apply_fixes_to_file_read_error() {
        // Create a temporary directory to ensure the non-existent file path is isolated
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("non_existent_file.txt");

        let diagnostics = vec![make_diagnostic_with_fix(0, 5, "Hi")];

        // This should fail to open the file
        let result = apply_fixes_to_file(&path, &diagnostics);

        match result {
            Err(LinterError::File(msg)) => {
                assert!(
                    msg.contains("Failed to open"),
                    "Unexpected error message: {}",
                    msg
                );
            }
            Ok(_) => panic!("Expected LinterError::File, got Ok"),
            Err(e) => panic!("Expected LinterError::File, got {:?}", e),
        }
    }

    #[test]
    fn apply_fixes_to_file_rejects_directory() {
        // Directories are not regular files and must be rejected.
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let diagnostics: Vec<Diagnostic> = vec![];
        let result = apply_fixes_to_file(dir.path(), &diagnostics);

        match result {
            Err(LinterError::File(msg)) => {
                assert!(
                    msg.contains("Not a regular file")
                        || msg.contains("Failed to open")
                        || msg.contains("Access is denied")
                        || msg.contains("Permission denied"),
                    "Unexpected error message: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected LinterError::File, got {:?}", e),
            Ok(_) => panic!("Expected error for directory input, got Ok"),
        }
    }

    #[test]
    fn apply_fixes_to_file_inner_rejects_oversized_metadata() {
        // Simulate a file whose metadata-reported size already exceeds the
        // limit; we must reject before ever issuing a read.
        use tempfile::NamedTempFile;
        let file = NamedTempFile::new().expect("Failed to create temp file");
        file.as_file().set_len(100).expect("Failed to set len");
        let path = file.into_temp_path();

        let res = apply_fixes_to_file_inner(&path, &[], 10);
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("exceeds limit"), "unexpected: {msg}");
    }

    #[test]
    fn apply_fixes_to_file_inner_rejects_oversized_content() {
        // Write more bytes than max_size — metadata check passes (size is
        // within limit), but the bounded read should then reject because the
        // +1 byte makes total > max_size.
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"123456").unwrap();
        let path = file.into_temp_path();

        let res = apply_fixes_to_file_inner(&path, &[], 3);
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("exceeds limit"), "unexpected: {msg}");
    }

    #[test]
    #[cfg(unix)]
    fn apply_fixes_to_file_inner_proc_file_is_bounded() {
        // /proc/version reports metadata.len() == 0 but produces many bytes
        // when read.  With max_size = 5, metadata check passes, then the
        // bounded read must reject the oversized content — this is the
        // OOM-guard we rely on for /dev/zero-style pseudo-files.
        let path = Path::new("/proc/version");
        if path.exists() {
            let res = apply_fixes_to_file_inner(path, &[], 5);
            assert!(res.is_err());
            assert!(res.unwrap_err().to_string().contains("exceeds limit"));
        }
    }

    /// Opening a FIFO must not hang the `--fix` path.  With `O_NONBLOCK` the
    /// open returns immediately, and `check_file_metadata` then rejects the
    /// FIFO because it is not a regular file — all bounded by a real
    /// wall-clock deadline to catch regressions.
    #[test]
    #[cfg(unix)]
    fn apply_fixes_to_file_fifo_does_not_block() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let fifo = dir.path().join("fifo");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: c_path is a valid nul-terminated string.
        let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) };
        if rc != 0 {
            eprintln!("mkfifo not supported; skipping");
            return;
        }

        let (tx, rx) = mpsc::channel();
        let fifo_path = fifo.clone();
        let handle = thread::spawn(move || {
            let res = apply_fixes_to_file(&fifo_path, &[]);
            let _ = tx.send(res);
        });

        // If the FIFO still blocks we'd hang here; 5s is generous but
        // finite, so a regression surfaces as a test timeout rather than a
        // hang.
        let res = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("apply_fixes_to_file blocked on FIFO");
        let _ = handle.join();

        match res {
            Err(LinterError::File(msg)) => {
                assert!(
                    msg.contains("Not a regular file") || msg.contains("Failed to open"),
                    "unexpected error: {msg}"
                );
            }
            Err(e) => panic!("Expected LinterError::File, got {e:?}"),
            Ok(_) => panic!("FIFO should not be accepted as a fixable file"),
        }
    }

    /// `/dev/zero` is a regular stream of NUL bytes.  Without the bounded
    /// read, `fs::read_to_string` would allocate unbounded memory and OOM.
    /// With the bounded read we get a clean `exceeds limit` error.
    #[test]
    #[cfg(unix)]
    fn apply_fixes_to_file_dev_zero_is_bounded() {
        let path = Path::new("/dev/zero");
        if !path.exists() {
            return;
        }
        let res = apply_fixes_to_file_inner(path, &[], 1024);
        match res {
            Err(LinterError::File(msg)) => {
                // Either "Not a regular file" (if metadata reports it as a
                // char device) or "exceeds limit" (if metadata passes but
                // the read overflows).  Both are safe outcomes; what we
                // must *not* see is an unbounded allocation.
                assert!(
                    msg.contains("Not a regular file")
                        || msg.contains("exceeds limit")
                        || msg.contains("Failed to read"),
                    "unexpected error for /dev/zero: {msg}"
                );
            }
            Err(e) => panic!("Expected LinterError::File, got {e:?}"),
            Ok(_) => panic!("/dev/zero must not be accepted as a fixable file"),
        }
    }

    /// Symlink-swap TOCTOU race: even if an attacker swaps the symlink
    /// target between metadata and open, the fix path reads from the fd we
    /// opened ourselves (via `open_nonblocking` + `file.metadata`), not by
    /// reopening the path.  This test just exercises the symlink-followed
    /// path to make sure a regular-file symlink is still accepted.
    #[test]
    #[cfg(unix)]
    fn apply_fixes_to_file_follows_symlink_to_regular_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::File::create(&real)
            .unwrap()
            .write_all(b"Hello World")
            .unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let diagnostics = vec![make_diagnostic_with_fix(6, 11, "Rust")];
        let result = apply_fixes_to_file(&link, &diagnostics).expect("symlink fix must succeed");
        assert!(result.modified);
        assert_eq!(result.fixed_content, "Hello Rust");

        // The real file on disk is updated (the symlink was followed).
        let on_disk = std::fs::read_to_string(&real).unwrap();
        assert_eq!(on_disk, "Hello Rust");
    }
}
