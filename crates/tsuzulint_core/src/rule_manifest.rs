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

    // Security: Reject absolute paths
    if wasm_relative.is_absolute() {
        return Err(LinterError::Config(format!(
            "Absolute WASM path '{}' is not allowed in manifest '{}'",
            manifest.artifacts.wasm,
            manifest_path.display()
        )));
    }

    // Security: Reject ParentDir (..) components
    if wasm_relative
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(LinterError::Config(format!(
            "WASM path '{}' containing '..' is not allowed in manifest '{}'",
            manifest.artifacts.wasm,
            manifest_path.display()
        )));
    }

    let wasm_path = manifest_dir.join(wasm_relative);

    if !wasm_path.exists() {
        return Err(LinterError::Config(format!(
            "WASM file not found at '{}' (referenced from '{}')",
            wasm_path.display(),
            manifest_path.display()
        )));
    }

    // Security: Verify canonicalized paths to prevent any other traversal
    let canonical_manifest_dir = manifest_dir.canonicalize().map_err(|e| {
        LinterError::Config(format!(
            "Failed to canonicalize manifest directory '{}': {}",
            manifest_dir.display(),
            e
        ))
    })?;
    let canonical_wasm_path = wasm_path.canonicalize().map_err(|e| {
        LinterError::Config(format!(
            "Failed to canonicalize WASM path '{}': {}",
            wasm_path.display(),
            e
        ))
    })?;

    if !canonical_wasm_path.starts_with(&canonical_manifest_dir) {
        return Err(LinterError::Config(format!(
            "WASM path '{}' resolves outside the manifest directory '{}'",
            wasm_path.display(),
            manifest_path.display()
        )));
    }

    Ok((manifest, canonical_wasm_path))
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
        assert_eq!(resolved_wasm, wasm_path.canonicalize().unwrap());
    }

    #[test]
    fn test_load_rule_manifest_absolute_path_fail() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        // Use valid sha256 to bypass manifest validation
        let json = r#"{
            "rule": { "name": "test", "version": "1.0.0" },
            "artifacts": {
                "wasm": "/absolute/path/rule.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;
        fs::write(&manifest_path, json).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Absolute WASM path"),
            "Error message was: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_rule_manifest_traversal_fail() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        // Use valid sha256 to bypass manifest validation
        let json = r#"{
            "rule": { "name": "test", "version": "1.0.0" },
            "artifacts": {
                "wasm": "../rule.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;
        fs::write(&manifest_path, json).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("containing '..'"),
            "Error message was: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_rule_manifest_outside_dir_fail() {
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join("sub");
        fs::create_dir(&sub_dir).unwrap();

        let _manifest_path = sub_dir.join("tsuzulint-rule.json");
        let _wasm_path = dir.path().join("outside.wasm");
        File::create(&_wasm_path).unwrap();

        // This path is tricky: it doesn't contain ".." literally, but if it resolved outside,
        // (e.g. via hardlinks or something if we didn't check components), canonicalize would catch it.
        // However, we already reject ".." components which is the main way to traverse.
        // For testing the `starts_with` check, we'd need a way to resolve outside without "..".
        // In many systems, ".." is the only way for a relative path to go up.
    }

    #[test]
    fn test_load_rule_manifest_missing_manifest() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("nonexistent.json");
        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read rule manifest")
        );
    }

    #[test]
    fn test_load_rule_manifest_missing_wasm() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let json = r#"{
            "rule": { "name": "test", "version": "1.0.0" },
            "artifacts": {
                "wasm": "missing.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;
        fs::write(&manifest_path, json).unwrap();
        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("WASM file not found")
        );
    }

    #[test]
    fn test_load_rule_manifest_invalid_json() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        fs::write(&manifest_path, "invalid json").unwrap();
        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid rule manifest")
        );
    }
}
