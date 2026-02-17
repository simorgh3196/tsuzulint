use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use predicates::prelude::*;

#[test]
fn test_rules_create_generates_valid_project() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    let rule_name = "test-rule-integration";

    let mut cmd = cargo_bin_cmd!("tzlint");
    cmd.current_dir(temp.path())
        .arg("rules")
        .arg("create")
        .arg(rule_name)
        .assert()
        .success();

    let rule_dir = temp.child(rule_name);
    rule_dir.assert(predicate::path::exists());
    rule_dir
        .child("Cargo.toml")
        .assert(predicate::path::exists());
    rule_dir
        .child("src/lib.rs")
        .assert(predicate::path::exists());

    // --- Cargo.toml checks ---
    let cargo_toml = std::fs::read_to_string(rule_dir.child("Cargo.toml").path())?;
    assert!(
        cargo_toml.contains("crate-type = [\"cdylib\"]"),
        "Cargo.toml must specify cdylib crate type for WASM"
    );
    assert!(
        cargo_toml.contains("extism-pdk"),
        "Cargo.toml must depend on extism-pdk"
    );
    assert!(
        cargo_toml.contains("rmp-serde"),
        "Cargo.toml must depend on rmp-serde for MessagePack"
    );

    // --- src/lib.rs checks ---
    let lib_rs = std::fs::read_to_string(rule_dir.child("src/lib.rs").path())?;

    // get_manifest uses JSON (String return type)
    assert!(
        lib_rs.contains("pub fn get_manifest() -> FnResult<String>"),
        "get_manifest must return String (JSON protocol)"
    );
    assert!(
        lib_rs.contains("serde_json::to_string(&manifest)"),
        "get_manifest must serialize as JSON"
    );

    // lint uses MessagePack (Vec<u8> in/out)
    assert!(
        lib_rs.contains("pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>>"),
        "lint must accept Vec<u8> (MessagePack protocol)"
    );
    assert!(
        lib_rs.contains("rmp_serde::from_slice"),
        "lint must deserialize request with rmp_serde"
    );
    assert!(
        lib_rs.contains("rmp_serde::to_vec_named"),
        "lint must serialize response with rmp_serde"
    );

    // Rule name is correctly embedded
    assert!(
        lib_rs.contains(&format!("\"{}\"", rule_name)),
        "Rule name must be embedded in generate code"
    );

    Ok(())
}

#[test]
fn test_rules_create_fails_on_existing_directory() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    let rule_name = "duplicate-rule";

    // Create the directory first
    std::fs::create_dir(temp.path().join(rule_name))?;

    let mut cmd = cargo_bin_cmd!("tzlint");
    cmd.current_dir(temp.path())
        .arg("rules")
        .arg("create")
        .arg(rule_name)
        .assert()
        .failure();

    Ok(())
}
