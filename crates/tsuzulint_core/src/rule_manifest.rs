use extism_manifest::Wasm;
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

pub fn load_rule_manifest(manifest_path: &Path) -> Result<LoadRuleManifestResult, LinterError> {
    use std::io::Read;
    let mut file = std::fs::File::open(manifest_path).map_err(|e| {
        LinterError::Config(format!(
            "Failed to open rule manifest '{}': {}",
            manifest_path.display(),
            e
        ))
    })?;

    let metadata = file.metadata().map_err(|e| {
        LinterError::Config(format!(
            "Failed to read metadata for rule manifest '{}': {}",
            manifest_path.display(),
            e
        ))
    })?;

    if metadata.len() > MAX_MANIFEST_SIZE {
        return Err(LinterError::Config(format!(
            "Rule manifest '{}' is too large (exceeds 10MB limit)",
            manifest_path.display()
        )));
    }

    let mut content = String::with_capacity(metadata.len() as usize);
    let bytes_read = (&mut file)
        .take(MAX_MANIFEST_SIZE + 1)
        .read_to_string(&mut content)
        .map_err(|e| {
            LinterError::Config(format!(
                "Failed to read rule manifest '{}': {}",
                manifest_path.display(),
                e
            ))
        })?;

    if bytes_read > MAX_MANIFEST_SIZE as usize {
        return Err(LinterError::Config(format!(
            "Rule manifest '{}' is too large (exceeds 10MB limit)",
            manifest_path.display()
        )));
    }

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

    let mut wasm_file = std::fs::File::open(&canonical_wasm_path).map_err(|e| {
        LinterError::Config(format!(
            "Failed to open WASM file '{}': {}",
            canonical_wasm_path.display(),
            e
        ))
    })?;

    let wasm_metadata = wasm_file.metadata().map_err(|e| {
        LinterError::Config(format!(
            "Failed to read metadata for WASM file '{}': {}",
            canonical_wasm_path.display(),
            e
        ))
    })?;

    if wasm_metadata.len() > tsuzulint_plugin::MAX_WASM_SIZE {
        return Err(LinterError::Config(format!(
            "WASM file '{}' is too large (exceeds {} byte limit)",
            canonical_wasm_path.display(),
            tsuzulint_plugin::MAX_WASM_SIZE
        )));
    }

    let mut wasm_bytes = Vec::with_capacity(wasm_metadata.len() as usize);
    let wasm_bytes_read = (&mut wasm_file)
        .take(tsuzulint_plugin::MAX_WASM_SIZE + 1)
        .read_to_end(&mut wasm_bytes)
        .map_err(|e| {
            LinterError::Config(format!(
                "Failed to read WASM file '{}': {}",
                canonical_wasm_path.display(),
                e
            ))
        })?;

    if wasm_bytes_read > tsuzulint_plugin::MAX_WASM_SIZE as usize {
        return Err(LinterError::Config(format!(
            "WASM file '{}' is too large (exceeds {} byte limit)",
            canonical_wasm_path.display(),
            tsuzulint_plugin::MAX_WASM_SIZE
        )));
    }

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
    use std::fs;
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
            "wasm": [{{
                "path": "rule.wasm",
                "hash": "{}"
            }}]
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
            "wasm": [{
                "path": "/absolute/path/rule.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
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
            "wasm": [{
                "path": "../rule.wasm",
                "hash": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
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

#[test]
fn test_load_rule_manifest_wasm_too_large() {
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("tsuzulint-rule.json");
    let wasm_path = dir.path().join("rule.wasm");

    let wasm_hash = "0000000000000000000000000000000000000000000000000000000000000000";
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
    File::create(&manifest_path)
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();

    let file = File::create(&wasm_path).unwrap();
    file.set_len(tsuzulint_plugin::MAX_WASM_SIZE + 1).unwrap();

    let result = load_rule_manifest(&manifest_path);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("is too large"));
}

#[cfg(test)]
#[cfg(unix)]
mod missing_coverage_tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    #[cfg(unix)]
    fn test_load_rule_manifest_read_error_fifo() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        // Create a FIFO so it can be opened, but blocks on read (we'll just drop it or let it fail)
        // Wait, if it blocks, the test hangs. Let's create a file with no read permissions instead.
        File::create(&manifest_path).unwrap();
        let mut perms = std::fs::metadata(&manifest_path).unwrap().permissions();
        perms.set_readonly(true); // this makes it read-only, not unreadable.

        // Let's use a directory as the manifest path. File::open on a directory returns an error on most OSs.
        // Actually File::open(dir) might work on Unix but read will fail.
        let dir_path = dir.path().join("manifest_dir.json");
        std::fs::create_dir(&dir_path).unwrap();

        let result = load_rule_manifest(&dir_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // It should either fail to open or fail to read
        assert!(
            err_msg.contains("Failed to open rule manifest")
                || err_msg.contains("Failed to read rule manifest")
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_load_rule_manifest_wasm_read_error() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_dir = dir.path().join("rule.wasm");
        std::fs::create_dir(&wasm_dir).unwrap(); // directory instead of file

        let wasm_hash = "0000000000000000000000000000000000000000000000000000000000000000";
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
        File::create(&manifest_path)
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to open WASM file")
                || err_msg.contains("Failed to read WASM file")
                || err_msg.contains("Failed to read metadata for WASM file")
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_load_rule_manifest_read_limit_bypass() {
        use std::thread;
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        // Create a FIFO which has 0 size according to metadata, but we can write > MAX_MANIFEST_SIZE bytes into it.
        unsafe {
            libc::mkfifo(
                std::ffi::CString::new(manifest_path.to_str().unwrap())
                    .unwrap()
                    .as_ptr(),
                0o666,
            )
        };

        let path_clone = manifest_path.clone();
        let handle = thread::spawn(move || {
            if let Ok(mut file) = File::create(&path_clone) {
                // Write MAX_MANIFEST_SIZE + 10 bytes
                let data = vec![0u8; (MAX_MANIFEST_SIZE + 10) as usize];
                let _ = file.write_all(&data);
            }
        });

        let result = load_rule_manifest(&manifest_path);
        // It should read MAX_MANIFEST_SIZE + 1 and then fail the bytes_read check
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is too large"));

        let _ = handle.join();
    }

    #[test]
    #[cfg(unix)]
    fn test_load_rule_manifest_wasm_limit_bypass() {
        use std::thread;
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        let wasm_hash = "0000000000000000000000000000000000000000000000000000000000000000";
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
        File::create(&manifest_path)
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();

        // Create a FIFO for WASM
        unsafe {
            libc::mkfifo(
                std::ffi::CString::new(wasm_path.to_str().unwrap())
                    .unwrap()
                    .as_ptr(),
                0o666,
            )
        };

        let path_clone = wasm_path.clone();
        let handle = thread::spawn(move || {
            if let Ok(mut file) = File::create(&path_clone) {
                // Write MAX_WASM_SIZE + 10 bytes
                let data = vec![0u8; (tsuzulint_plugin::MAX_WASM_SIZE + 10) as usize];
                let _ = file.write_all(&data);
            }
        });

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("is too large"),
            "Expected size error but got: {}",
            err_msg
        );

        let _ = handle.join();
    }
}
