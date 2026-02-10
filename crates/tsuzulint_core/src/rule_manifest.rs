use std::fs;
use std::path::{Path, PathBuf};
use tsuzulint_manifest::{ExternalRuleManifest, validate_manifest};

use crate::error::LinterError;

/// Loads a rule manifest from a file.
///
/// Returns the parsed manifest and the resolved path to the WASM file.
pub fn load_rule_manifest(
    manifest_path: &Path,
) -> Result<(ExternalRuleManifest, PathBuf), LinterError> {
    let content = fs::read_to_string(manifest_path).map_err(|e| {
        LinterError::Config(format!(
            "Failed to read rule manifest '{}': {}",
            manifest_path.display(),
            e
        ))
    })?;

    let manifest = validate_manifest(&content).map_err(|e| {
        LinterError::Config(format!(
            "Invalid rule manifest '{}': {}",
            manifest_path.display(),
            e
        ))
    })?;

    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    // Resolve WASM path relative to the manifest file
    let wasm_relative = Path::new(&manifest.artifacts.wasm);
    let wasm_path = manifest_dir.join(wasm_relative);

    if !wasm_path.exists() {
        return Err(LinterError::Config(format!(
            "WASM file not found at '{}' (referenced from '{}')",
            wasm_path.display(),
            manifest_path.display()
        )));
    }

    Ok((manifest, wasm_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_load_rule_manifest_success() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        // Create dummy WASM file
        File::create(&wasm_path).unwrap().write_all(b"").unwrap();

        // Create manifest
        let json = r#"{
            "rule": {
                "name": "test-rule",
                "version": "1.0.0",
                "description": "Test rule",
                "fixable": false
            },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;
        fs::write(&manifest_path, json).unwrap();

        let (manifest, resolved_wasm) = load_rule_manifest(&manifest_path).unwrap();

        assert_eq!(manifest.rule.name, "test-rule");
        assert_eq!(resolved_wasm, wasm_path);
    }
}
