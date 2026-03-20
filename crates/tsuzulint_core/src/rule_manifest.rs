use extism_manifest::Wasm;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
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

const MAX_MANIFEST_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_WASM_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

pub fn load_rule_manifest(manifest_path: &Path) -> Result<LoadRuleManifestResult, LinterError> {
    let read_secure = |path: &Path, max_size: u64, kind: &str| -> Result<Vec<u8>, LinterError> {
        let mut file = File::open(path).map_err(|e| {
            LinterError::Config(format!("Failed to open {} '{}': {}", kind, path.display(), e))
        })?;

        let metadata = file.metadata().map_err(|e| {
            LinterError::Config(format!("Failed to read metadata for {} '{}': {}", kind, path.display(), e))
        })?;

        let len = metadata.len();
        if len > max_size {
            return Err(LinterError::Config(format!(
                "{} '{}' is too large (exceeds {} bytes limit)",
                kind,
                path.display(),
                max_size
            )));
        }

        let mut content = Vec::with_capacity(len as usize);
        (&mut file).take(max_size + 1).read_to_end(&mut content).map_err(|e| {
            LinterError::Config(format!("Failed to read {} '{}': {}", kind, path.display(), e))
        })?;

        if content.len() as u64 > max_size {
            return Err(LinterError::Config(format!(
                "{} '{}' is too large (exceeds {} bytes limit)",
                kind,
                path.display(),
                max_size
            )));
        }

        Ok(content)
    };

    let content_bytes = read_secure(manifest_path, MAX_MANIFEST_SIZE, "rule manifest")?;
    let content = String::from_utf8(content_bytes).map_err(|e| {
        LinterError::Config(format!(
            "Rule manifest '{}' contains invalid UTF-8: {}",
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

    let mut resolved_wasm = None;
    for w in &manifest.wasm {
        match w {
            Wasm::File { path, meta } => {
                resolved_wasm = Some((path.clone(), meta.hash.clone()));
                break;
            }
            Wasm::Url { .. } => {
                return Err(LinterError::Config(format!(
                    "URL WASM source is not supported directly in core; please resolve it via registry first (manifest '{}')",
                    manifest_path.display()
                )));
            }
            Wasm::Data { .. } => {
                // Ignore Data sources and continue searching for File sources.
                continue;
            }
        }
    }

    let (wasm_path_buf, expected_hash): (PathBuf, Option<String>) =
        resolved_wasm.ok_or_else(|| {
            LinterError::Config(format!(
                "No valid WASM source found in manifest '{}'",
                manifest_path.display()
            ))
        })?;

    let expected_hash = expected_hash.ok_or_else(|| {
        LinterError::Config(format!(
            "Missing hash for WASM source in manifest '{}'",
            manifest_path.display()
        ))
    })?;

    let wasm_relative = wasm_path_buf.as_path();

    if wasm_relative.is_absolute() || wasm_relative.has_root() {
        return Err(LinterError::Config(format!(
            "Absolute WASM path '{}' is not allowed in manifest '{}'",
            wasm_relative.display(),
            manifest_path.display()
        )));
    }

    if wasm_relative
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(LinterError::Config(format!(
            "WASM path '{}' containing '..' is not allowed in manifest '{}'",
            wasm_relative.display(),
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

    let wasm_bytes = read_secure(&canonical_wasm_path, MAX_WASM_SIZE, "WASM file")?;

    HashVerifier::verify(&wasm_bytes, &expected_hash).map_err(|e| {
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
            "wasm": [{{
                "path": "rule.wasm",
                "hash": "{}"
            }}]
        }}"#,
            wasm_hash
        );
        std::fs::write(&manifest_path, json).unwrap();

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
            "wasm": [{{
                "path": "rule.wasm",
                "hash": "{}"
            }}]
        }}"#,
            wrong_hash
        );
        std::fs::write(&manifest_path, json).unwrap();

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
            "wasm": [{
                "path": "/absolute/path/rule.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        }"#;
        std::fs::write(&manifest_path, json).unwrap();

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
            "wasm": [{
                "path": "../rule.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        }"#;
        std::fs::write(&manifest_path, json).unwrap();

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
        std::fs::create_dir(&sub_dir).unwrap();

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
                .contains("Failed to open rule manifest")
        );
    }

    #[test]
    fn test_load_rule_manifest_missing_wasm() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let json = r#"{
            "rule": { "name": "test", "version": "1.0.0" },
            "wasm": [{
                "path": "missing.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        }"#;
        std::fs::write(&manifest_path, json).unwrap();
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
    fn test_load_rule_manifest_wasm_too_large() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        let file = File::create(&wasm_path).unwrap();
        file.set_len(MAX_WASM_SIZE + 1).unwrap();

        let json = r#"{
            "rule": { "name": "test", "version": "1.0.0" },
            "wasm": [{
                "path": "rule.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        }"#.to_string();
        std::fs::write(&manifest_path, json).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("is too large"),
            "Expected error about size, but got: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_rule_manifest_invalid_utf8() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        // Write invalid UTF-8 bytes (0xFF is invalid in UTF-8)
        std::fs::write(&manifest_path, &[0xFF, 0xFF, 0xFF]).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("contains invalid UTF-8"),
            "Expected error about UTF-8, but got: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_rule_manifest_invalid_json() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        std::fs::write(&manifest_path, "invalid json").unwrap();
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

#[cfg(test)]
mod extra_tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_load_rule_manifest_too_large() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        let file = File::create(&manifest_path).unwrap();
        // create a file that's exactly MAX_MANIFEST_SIZE + 1 bytes long
        file.set_len(MAX_MANIFEST_SIZE + 1).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is too large"));
    }

    #[test]
    fn test_load_rule_manifest_at_size_limit() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        let file = File::create(&manifest_path).unwrap();
        // create a file that's exactly MAX_MANIFEST_SIZE bytes long
        file.set_len(MAX_MANIFEST_SIZE).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err()); // Will fail to parse as JSON, but shouldn't fail size limit
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.contains("is too large"),
            "Expected error not to be about size, but got: {}",
            err_msg
        );
    }
}
