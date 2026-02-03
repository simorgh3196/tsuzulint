//! Integration tests for CLI behavior
//!
//! These tests verify the external behavior of the CLI tool,
//! following behavior-driven testing principles.

use assert_cmd::Command;
use predicates::prelude::*;

/// Helper to create a command for the tzlint CLI
fn tsuzulint_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tzlint"));
    cmd.arg("--no-cache");
    cmd
}

mod help_command {
    use super::*;

    #[test]
    fn shows_help_with_flag() {
        tsuzulint_cmd()
            .arg("--help")
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage:"));
    }

    #[test]
    fn shows_version_with_flag() {
        tsuzulint_cmd()
            .arg("--version")
            .assert()
            .success()
            .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }
}

mod lint_command {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn lints_markdown_file() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .assert()
            .success();
    }

    #[test]
    fn lints_plain_text_file() {
        let sample_txt = fixtures_dir().join("sample.txt");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_txt)
            .assert()
            .success();
    }

    #[test]
    fn reports_zero_files_for_nonexistent_path() {
        tsuzulint_cmd()
            .arg("lint")
            .arg("nonexistent_file.md")
            .assert()
            .success()
            .stdout(predicate::str::contains("Checked 0 files"));
    }
}
