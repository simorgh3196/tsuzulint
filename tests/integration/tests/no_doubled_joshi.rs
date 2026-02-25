//! Integration tests for no-doubled-joshi rule
//!
//! Tests the full linting pipeline including morphological analysis via Lindera.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/japanese")
}

fn tsuzulint_cmd() -> Command {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to find workspace root");
    let bin_path = workspace_root.join("target/debug/tzlint");
    let mut cmd = Command::new(bin_path);
    cmd.arg("--no-cache");
    cmd
}

mod valid_cases {
    use super::*;

    #[test]
    fn allows_no_exception() {
        let fixture = fixtures_dir().join("valid_no_exception.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .success()
            .stdout(predicate::str::contains("助詞").not());
    }

    #[test]
    fn allows_wo_exception() {
        let fixture = fixtures_dir().join("valid_wo_exception.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .success()
            .stdout(predicate::str::contains("助詞").not());
    }

    #[test]
    fn allows_te_exception() {
        let fixture = fixtures_dir().join("valid_te_exception.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .success()
            .stdout(predicate::str::contains("助詞").not());
    }

    #[test]
    fn allows_comma_interval() {
        let fixture = fixtures_dir().join("valid_comma_interval.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .success()
            .stdout(predicate::str::contains("助詞").not());
    }

    #[test]
    fn allows_parallel_particles() {
        let fixture = fixtures_dir().join("valid_parallel.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .success()
            .stdout(predicate::str::contains("助詞").not());
    }

    #[test]
    fn allows_ka_douka_pattern() {
        let fixture = fixtures_dir().join("valid_ka_douka.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .success()
            .stdout(predicate::str::contains("助詞").not());
    }
}

mod invalid_cases {
    use super::*;

    #[test]
    fn detects_doubled_ha() {
        let fixture = fixtures_dir().join("invalid_doubled_ha.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .stdout(predicate::str::contains(
                "一文に二回以上利用されている助詞 \"は\"",
            ));
    }

    #[test]
    fn detects_doubled_de() {
        let fixture = fixtures_dir().join("invalid_doubled_de.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .stdout(predicate::str::contains(
                "一文に二回以上利用されている助詞 \"で\"",
            ));
    }

    #[test]
    fn detects_doubled_rengo() {
        let fixture = fixtures_dir().join("invalid_doubled_rengo.md");

        tsuzulint_cmd()
            .arg("lint")
            .arg(&fixture)
            .assert()
            .stdout(predicate::str::contains(
                "一文に二回以上利用されている助詞 \"には\"",
            ));
    }
}
