//! Integration tests for CLI commands
//!
//! Tests for init, rules, and other CLI commands.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper to create a command for the tzlint CLI
fn tsuzulint_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tzlint"));
    cmd.arg("--no-cache");
    cmd
}

/// Helper to get fixtures directory
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

mod init_command {
    use super::*;

    #[test]
    fn creates_new_config_file() {
        let temp_dir = TempDir::new().unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("init")
            .assert()
            .success()
            .stderr(predicate::str::contains("Created .tsuzulint.jsonc"));

        // Verify config file was created
        let config_path = temp_dir.path().join(".tsuzulint.jsonc");
        assert!(config_path.exists());

        // Verify config has default structure
        let content = fs::read_to_string(config_path).unwrap();
        assert!(content.contains("rules"));
        assert!(content.contains("options"));
        assert!(content.contains("cache"));
    }

    #[test]
    fn fails_when_config_exists_without_force() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".tsuzulint.jsonc");

        // Create existing config
        fs::write(&config_path, "{}").unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("init")
            .assert()
            .failure()
            .stderr(predicate::str::contains("already exists"));
    }

    #[test]
    fn overwrites_config_with_force() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".tsuzulint.jsonc");

        // Create existing config with custom content
        fs::write(&config_path, r#"{"custom": "data"}"#).unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("init")
            .arg("--force")
            .assert()
            .success();

        // Verify config was overwritten
        let content = fs::read_to_string(config_path).unwrap();
        assert!(!content.contains("custom"));
        assert!(content.contains("rules"));
    }
}

mod rules_commands {
    use super::*;

    #[test]
    fn create_command_generates_rule_project() {
        let temp_dir = TempDir::new().unwrap();
        let rule_name = "test-rule";

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("rules")
            .arg("create")
            .arg(rule_name)
            .assert()
            .success()
            .stderr(predicate::str::contains("Created rule project: test-rule"));

        // Verify directory structure
        let rule_dir = temp_dir.path().join(rule_name);
        assert!(rule_dir.exists());
        assert!(rule_dir.join("Cargo.toml").exists());
        assert!(rule_dir.join("src/lib.rs").exists());

        // Verify Cargo.toml content
        let cargo_content = fs::read_to_string(rule_dir.join("Cargo.toml")).unwrap();
        assert!(cargo_content.contains("cdylib"));
        assert!(cargo_content.contains("extism-pdk"));

        // Verify lib.rs has necessary functions
        let lib_content = fs::read_to_string(rule_dir.join("src/lib.rs")).unwrap();
        assert!(lib_content.contains("get_manifest"));
        assert!(lib_content.contains("lint"));
    }

    #[test]
    fn create_command_fails_if_directory_exists() {
        let temp_dir = TempDir::new().unwrap();
        let rule_name = "existing-rule";
        let rule_dir = temp_dir.path().join(rule_name);

        // Create the directory first
        fs::create_dir(&rule_dir).unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("rules")
            .arg("create")
            .arg(rule_name)
            .assert()
            .failure()
            .stderr(predicate::str::contains("already exists"));
    }

    #[test]
    fn add_command_fails_for_nonexistent_file() {
        tsuzulint_cmd()
            .arg("rules")
            .arg("add")
            .arg("/nonexistent/rule.wasm")
            .assert()
            .failure()
            .stderr(predicate::str::contains("File not found"));
    }

    #[test]
    fn add_command_succeeds_for_valid_file() {
        let temp_dir = TempDir::new().unwrap();
        let wasm_file = temp_dir.path().join("dummy.wasm");

        // Create a dummy file (doesn't need to be valid WASM for this test)
        fs::write(&wasm_file, b"dummy wasm content").unwrap();

        tsuzulint_cmd()
            .arg("rules")
            .arg("add")
            .arg(&wasm_file)
            .assert()
            .success()
            .stderr(predicate::str::contains("Rule added"));
    }
}

mod lint_command_formats {
    use super::*;

    #[test]
    fn outputs_json_format() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .arg("--format")
            .arg("json")
            .assert()
            .success()
            .stdout(predicate::str::contains("path").and(predicate::str::contains("diagnostics")));
    }

    #[test]
    fn outputs_sarif_format() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .arg("--format")
            .arg("sarif")
            .assert()
            .success()
            .stdout(predicate::str::contains("$schema").and(predicate::str::contains("version")));
    }

    #[test]
    fn outputs_text_format_by_default() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .assert()
            .success()
            .stdout(predicate::str::contains("Checked").and(predicate::str::contains("files")));
    }
}

mod lint_command_fix {
    use super::*;

    #[test]
    fn dry_run_shows_would_fix() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test\nSome content").unwrap();

        // Since we have no rules loaded, this will just show "No fixable issues found"
        tsuzulint_cmd()
            .arg("lint")
            .arg(&test_file)
            .arg("--fix")
            .arg("--dry-run")
            .assert()
            .success()
            .stdout(predicate::str::contains("No fixable issues found")
                .or(predicate::str::contains("Would fix")));
    }

    #[test]
    fn fix_without_dry_run_processes() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test\nSome content").unwrap();

        tsuzulint_cmd()
            .arg("lint")
            .arg(&test_file)
            .arg("--fix")
            .assert()
            .success();

        // File should still exist
        assert!(test_file.exists());
    }

    #[test]
    fn dry_run_requires_fix_flag() {
        let sample_md = fixtures_dir().join("sample.md");

        // Should fail since --dry-run requires --fix
        let result = tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .arg("--dry-run")
            .assert()
            .failure();

        // clap will output an error about required argument
        result.stderr(predicate::str::contains("required").or(predicate::str::contains("requires")));
    }
}

mod lint_command_timings {
    use super::*;

    #[test]
    fn shows_performance_timings_when_requested() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .arg("--timings")
            .assert()
            .success()
            .stdout(predicate::str::contains("Performance Timings").or(
                // May not show if no rules are loaded
                predicate::str::contains("Checked")
            ));
    }
}

mod plugin_commands {
    use super::*;

    #[test]
    fn cache_clean_succeeds() {
        tsuzulint_cmd()
            .arg("plugin")
            .arg("cache")
            .arg("clean")
            .assert()
            .success()
            .stderr(predicate::str::contains("Plugin cache cleaned"));
    }

    #[test]
    fn install_requires_spec_or_url() {
        tsuzulint_cmd()
            .arg("plugin")
            .arg("install")
            .assert()
            .failure()
            .stderr(predicate::str::contains("Must provide a plugin spec or --url"));
    }

    #[test]
    fn install_url_requires_alias() {
        tsuzulint_cmd()
            .arg("plugin")
            .arg("install")
            .arg("--url")
            .arg("https://example.com/rule.wasm")
            .assert()
            .failure()
            .stderr(predicate::str::contains("--as"));
    }

    #[test]
    fn install_cannot_specify_both_spec_and_url() {
        tsuzulint_cmd()
            .arg("plugin")
            .arg("install")
            .arg("owner/repo")
            .arg("--url")
            .arg("https://example.com/rule.wasm")
            .assert()
            .failure()
            .stderr(predicate::str::contains("Cannot specify both"));
    }
}

mod config_file_handling {
    use super::*;

    #[test]
    fn uses_default_config_when_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test").unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("lint")
            .arg(&test_file)
            .assert()
            .success()
            .stderr(predicate::str::contains("No config file found, using defaults"));
    }

    #[test]
    fn loads_config_from_flag() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("custom.json");
        let test_file = temp_dir.path().join("test.md");

        fs::write(&config_file, r#"{"cache": false}"#).unwrap();
        fs::write(&test_file, "# Test").unwrap();

        tsuzulint_cmd()
            .arg("--config")
            .arg(&config_file)
            .arg("lint")
            .arg(&test_file)
            .assert()
            .success();
    }

    #[test]
    fn fails_on_invalid_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("invalid.json");
        let test_file = temp_dir.path().join("test.md");

        fs::write(&config_file, "invalid json").unwrap();
        fs::write(&test_file, "# Test").unwrap();

        tsuzulint_cmd()
            .arg("--config")
            .arg(&config_file)
            .arg("lint")
            .arg(&test_file)
            .assert()
            .failure();
    }
}

mod verbose_output {
    use super::*;

    #[test]
    fn enables_verbose_logging() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("--verbose")
            .arg("lint")
            .arg(&sample_md)
            .assert()
            .success();
        // With verbose, we should see more detailed output in stderr
        // But exact output depends on tracing configuration
    }
}

mod exit_codes {
    use super::*;

    #[test]
    fn exits_with_zero_on_success_no_errors() {
        let sample_md = fixtures_dir().join("sample.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&sample_md)
            .assert()
            .code(0);
    }

    #[test]
    fn exits_with_two_on_internal_error() {
        // Invalid glob pattern should cause an internal error
        tsuzulint_cmd()
            .arg("lint")
            .arg("[invalid-glob")
            .assert()
            .code(2);
    }
}

mod pattern_matching {
    use super::*;

    #[test]
    fn lints_multiple_files_with_glob() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file1.md"), "# File 1").unwrap();
        fs::write(temp_dir.path().join("file2.md"), "# File 2").unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("lint")
            .arg("*.md")
            .assert()
            .success()
            .stdout(predicate::str::contains("Checked 2 files"));
    }

    #[test]
    fn handles_recursive_glob() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();
        fs::write(temp_dir.path().join("root.md"), "# Root").unwrap();
        fs::write(sub_dir.join("nested.md"), "# Nested").unwrap();

        tsuzulint_cmd()
            .current_dir(temp_dir.path())
            .arg("lint")
            .arg("**/*.md")
            .assert()
            .success()
            .stdout(predicate::str::contains("Checked 2 files"));
    }
}