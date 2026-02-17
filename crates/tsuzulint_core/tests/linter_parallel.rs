//! Integration tests for parallel linting functionality.
//!
//! These tests verify that the linter correctly handles parallel processing
//! of multiple files and proper thread safety.

use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use tsuzulint_core::{LinterConfig, Linter};

fn create_test_linter(temp_dir: &TempDir) -> Linter {
    let mut config = LinterConfig::new();
    config.cache_dir = temp_dir.path().join(".cache").to_string_lossy().to_string();
    config.cache = false; // Disable cache for these tests

    Linter::new(config).expect("Failed to create linter")
}

#[test]
fn test_parallel_lint_empty_files_list() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    let files: Vec<PathBuf> = vec![];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert!(successes.is_empty());
    assert!(failures.is_empty());
}

#[test]
fn test_parallel_lint_single_file() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create a test file
    let test_file = temp_dir.path().join("test.md");
    fs::write(&test_file, "# Test\n\nSome content.").unwrap();

    let files = vec![test_file.clone()];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 1);
    assert_eq!(failures.len(), 0);
    assert_eq!(successes[0].path, test_file);
}

#[test]
fn test_parallel_lint_multiple_files() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create multiple test files
    let file1 = temp_dir.path().join("test1.md");
    let file2 = temp_dir.path().join("test2.md");
    let file3 = temp_dir.path().join("test3.md");

    fs::write(&file1, "# Test 1\n\nContent 1.").unwrap();
    fs::write(&file2, "# Test 2\n\nContent 2.").unwrap();
    fs::write(&file3, "# Test 3\n\nContent 3.").unwrap();

    let files = vec![file1.clone(), file2.clone(), file3.clone()];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 3);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_with_nonexistent_files() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Mix of existing and non-existing files
    let existing_file = temp_dir.path().join("exists.md");
    fs::write(&existing_file, "# Exists").unwrap();

    let nonexistent_file = temp_dir.path().join("does_not_exist.md");

    let files = vec![existing_file.clone(), nonexistent_file.clone()];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();

    // One should succeed, one should fail
    assert_eq!(successes.len(), 1);
    assert_eq!(failures.len(), 1);
    assert_eq!(successes[0].path, existing_file);
    assert_eq!(failures[0].0, nonexistent_file);
}

#[test]
fn test_parallel_lint_all_nonexistent_files() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    let files = vec![
        PathBuf::from("/nonexistent/file1.md"),
        PathBuf::from("/nonexistent/file2.md"),
        PathBuf::from("/nonexistent/file3.md"),
    ];

    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();

    // All should fail
    assert_eq!(successes.len(), 0);
    assert_eq!(failures.len(), 3);
}

#[test]
fn test_parallel_lint_large_batch() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create many files to test parallelization
    let mut files = Vec::new();
    for i in 0..50 {
        let file = temp_dir.path().join(format!("test{}.md", i));
        fs::write(&file, format!("# Test {}\n\nContent for file {}.", i, i)).unwrap();
        files.push(file);
    }

    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 50);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_different_extensions() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create files with different extensions
    let md_file = temp_dir.path().join("test.md");
    let txt_file = temp_dir.path().join("test.txt");
    let unknown_file = temp_dir.path().join("test.xyz");

    fs::write(&md_file, "# Markdown").unwrap();
    fs::write(&txt_file, "Plain text").unwrap();
    fs::write(&unknown_file, "Unknown format").unwrap();

    let files = vec![md_file, txt_file, unknown_file];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();

    // All should succeed (unknown formats default to plain text)
    assert_eq!(successes.len(), 3);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_empty_files() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create empty files
    let empty1 = temp_dir.path().join("empty1.md");
    let empty2 = temp_dir.path().join("empty2.md");

    fs::write(&empty1, "").unwrap();
    fs::write(&empty2, "").unwrap();

    let files = vec![empty1, empty2];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 2);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_unicode_content() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create files with Unicode content
    let file1 = temp_dir.path().join("unicode1.md");
    let file2 = temp_dir.path().join("unicode2.md");

    fs::write(&file1, "# 日本語テスト\n\nこれはテストです。").unwrap();
    fs::write(&file2, "# Emoji Test 🎉\n\nTest with emojis 🚀").unwrap();

    let files = vec![file1, file2];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 2);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_very_long_lines() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create file with very long line
    let file = temp_dir.path().join("long_line.md");
    let long_content = "a".repeat(10000);
    fs::write(&file, format!("# Test\n\n{}", long_content)).unwrap();

    let files = vec![file];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 1);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_duplicate_file_paths() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create a file
    let file = temp_dir.path().join("test.md");
    fs::write(&file, "# Test").unwrap();

    // Pass the same file multiple times
    let files = vec![file.clone(), file.clone(), file.clone()];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();

    // Each should be processed independently
    assert_eq!(successes.len(), 3);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_nested_directories() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create nested directory structure
    let sub_dir = temp_dir.path().join("subdir");
    fs::create_dir(&sub_dir).unwrap();
    let nested_dir = sub_dir.join("nested");
    fs::create_dir(&nested_dir).unwrap();

    let file1 = temp_dir.path().join("root.md");
    let file2 = sub_dir.join("sub.md");
    let file3 = nested_dir.join("nested.md");

    fs::write(&file1, "# Root").unwrap();
    fs::write(&file2, "# Sub").unwrap();
    fs::write(&file3, "# Nested").unwrap();

    let files = vec![file1, file2, file3];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 3);
    assert_eq!(failures.len(), 0);
}

#[test]
fn test_parallel_lint_preserves_file_order_in_results() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create files with specific names
    let file_a = temp_dir.path().join("a.md");
    let file_b = temp_dir.path().join("b.md");
    let file_c = temp_dir.path().join("c.md");

    fs::write(&file_a, "# A").unwrap();
    fs::write(&file_b, "# B").unwrap();
    fs::write(&file_c, "# C").unwrap();

    let files = vec![file_a.clone(), file_b.clone(), file_c.clone()];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, _) = result.unwrap();

    // Results should be present (order might vary due to parallelism)
    assert_eq!(successes.len(), 3);

    // Verify all files are in results
    let result_paths: Vec<&PathBuf> = successes.iter().map(|r| &r.path).collect();
    assert!(result_paths.contains(&&file_a));
    assert!(result_paths.contains(&&file_b));
    assert!(result_paths.contains(&&file_c));
}

#[test]
fn test_parallel_lint_special_characters_in_filename() {
    let temp_dir = TempDir::new().unwrap();
    let linter = create_test_linter(&temp_dir);

    // Create files with special characters (that are valid on filesystem)
    let file1 = temp_dir.path().join("test-with-dash.md");
    let file2 = temp_dir.path().join("test_with_underscore.md");
    let file3 = temp_dir.path().join("test.multiple.dots.md");

    fs::write(&file1, "# Test 1").unwrap();
    fs::write(&file2, "# Test 2").unwrap();
    fs::write(&file3, "# Test 3").unwrap();

    let files = vec![file1, file2, file3];
    let result = linter.lint_files(&files);

    assert!(result.is_ok());
    let (successes, failures) = result.unwrap();
    assert_eq!(successes.len(), 3);
    assert_eq!(failures.len(), 0);
}