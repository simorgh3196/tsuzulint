//! Integration tests for the `rules add` command

use std::path::PathBuf;
use std::process::Command;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tzlint"))
}

#[test]
fn test_add_rule_missing_file() {
    let output = Command::new(bin_path())
        .args(["rules", "add", "nonexistent.wasm"])
        .output()
        .expect("Failed to run tzlint");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found") || stderr.contains("File not found"));
}

#[test]
fn test_add_rule_invalid_extension() {
    let output = Command::new(bin_path())
        .args(["rules", "add", "Cargo.toml"])
        .output()
        .expect("Failed to run tzlint");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(".wasm") || stderr.contains("extension"));
}
