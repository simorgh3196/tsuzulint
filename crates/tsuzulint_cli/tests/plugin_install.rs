use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;
use tsuzulint_registry::hash::HashVerifier;

fn tsuzulint_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tzlint"));
    cmd.arg("--no-cache");
    cmd
}

#[test]
fn test_plugin_install_local_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // 1. Create WASM file
    let wasm_content = b"local wasm content";
    let wasm_path = root.join("rule.wasm");
    fs::write(&wasm_path, wasm_content).unwrap();

    // 2. Compute Hash
    let sha256 = HashVerifier::compute(wasm_content);

    // 3. Create Manifest
    let manifest_path = root.join("tsuzulint-rule.json");
    let manifest_content = format!(
        r#"
    {{
        "rule": {{
            "name": "local-rule",
            "version": "0.1.0"
        }},
        "artifacts": {{
            "wasm": "rule.wasm",
            "sha256": "{}"
        }}
    }}
    "#,
        sha256
    );
    fs::write(&manifest_path, manifest_content).unwrap();

    // 4. Create empty config (optional, but ensures we control the environment)
    let config_path = root.join(".tsuzulint.json");
    fs::write(&config_path, r#"{ "rules": [] }"#).unwrap();

    // 5. Construct JSON spec for local path
    // We need absolute path for the test execution to always find it
    let manifest_path_abs = manifest_path.to_string_lossy();
    // Escape backslashes for JSON on Windows and other special characters
    let path_json = serde_json::to_string(&manifest_path_abs).unwrap();
    let spec_json = format!(r#"{{"path": {}, "as": "my-local-rule"}}"#, path_json);

    // 6. Run install command
    // We run in 'root' so .tsuzulint.json is found/created there?
    // CLI `run_plugin_install` uses `.tsuzulint.jsonc` or `.tsuzulint.json` in CURRENT DIRECTORY.
    // So we must set current_dir to `root`.

    tsuzulint_cmd()
        .current_dir(root)
        .arg("plugin")
        .arg("install")
        .arg(&spec_json)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Successfully installed: local-rule",
        ));

    // 7. Verify config update
    let updated_config = fs::read_to_string(&config_path).unwrap();
    assert!(updated_config.contains("my-local-rule"));
    assert!(updated_config.contains("path"));

    // 8. Verify options update (default true)
    // We didn't provide defaults in manifest, so it should be true
    assert!(updated_config.contains(r#""my-local-rule": true"#));
}
