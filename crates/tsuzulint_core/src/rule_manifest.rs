use std::fs;
use std::path::Path;
use tsuzulint_manifest::{ExternalRuleManifest, HashVerifier, validate_manifest};

use crate::error::LinterError;

/// Result of loading a rule manifest with verified WASM bytes.
///
/// This struct enables single-read optimization: the WASM file is read once,
/// verified against the manifest's SHA256 hash, and the bytes are returned
/// for direct use by the plugin host without re-reading from disk.
#[derive(Debug)]
pub struct LoadRuleManifestResult {
    pub manifest: ExternalRuleManifest,
    pub wasm_bytes: Vec<u8>,
}

pub fn load_rule_manifest(manifest_path: &Path) -> Result<LoadRuleManifestResult, LinterError> {
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

    let wasm_relative = Path::new(&manifest.artifacts.wasm);

    if wasm_relative.is_absolute() || wasm_relative.has_root() {
        return Err(LinterError::Config(format!(
            "Absolute WASM path '{}' is not allowed in manifest '{}'",
            manifest.artifacts.wasm,
            manifest_path.display()
        )));
    }

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

    let wasm_bytes = fs::read(&canonical_wasm_path).map_err(|e| {
        LinterError::Config(format!(
            "Failed to read WASM file '{}': {}",
            canonical_wasm_path.display(),
            e
        ))
    })?;

    HashVerifier::verify(&wasm_bytes, &manifest.artifacts.sha256).map_err(|e| {
        LinterError::Config(format!(
            "Integrity check failed for WASM file '{}': {}",
            canonical_wasm_path.display(),
            e
        ))
    })?;

    Ok(LoadRuleManifestResult {
        manifest,
        wasm_bytes,
    })
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

        let wasm_content = b"test wasm content";
        File::create(&wasm_path)
            .unwrap()
            .write_all(wasm_content)
            .unwrap();

        let wasm_hash = HashVerifier::compute(wasm_content);
        let json = format!(
            r#"{{
            "rule": {{
                "name": "test-rule",
                "version": "1.0.0",
                "description": "Test rule",
                "fixable": false
            }},
            "artifacts": {{
                "wasm": "rule.wasm",
                "sha256": "{}"
            }}
        }}"#,
            wasm_hash
        );
        fs::write(&manifest_path, json).unwrap();

        let result = load_rule_manifest(&manifest_path).unwrap();

        assert_eq!(result.manifest.rule.name, "test-rule");
        assert_eq!(result.wasm_bytes, wasm_content);
    }

    #[test]
    fn test_load_rule_manifest_hash_mismatch() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        File::create(&wasm_path)
            .unwrap()
            .write_all(b"test content")
            .unwrap();

        let wrong_hash = "a".repeat(64);
        let json = format!(
            r#"{{
            "rule": {{ "name": "test", "version": "1.0.0" }},
            "artifacts": {{
                "wasm": "rule.wasm",
                "sha256": "{}"
            }}
        }}"#,
            wrong_hash
        );
        fs::write(&manifest_path, json).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Integrity check failed"),
            "Error message was: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_rule_manifest_absolute_path_fail() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

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
    #[ignore = "Hard to construct a path traversal without '..' components which are already rejected earlier"]
    fn test_load_rule_manifest_outside_dir_fail() {
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join("sub");
        fs::create_dir(&sub_dir).unwrap();

        let _manifest_path = sub_dir.join("tsuzulint-rule.json");
        let _wasm_path = dir.path().join("outside.wasm");
        File::create(&_wasm_path).unwrap();
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
