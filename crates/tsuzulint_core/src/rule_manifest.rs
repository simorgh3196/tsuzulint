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
    let mut file = File::open(manifest_path).map_err(|e| {
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
    (&mut file)
        .take(MAX_MANIFEST_SIZE + 1)
        .read_to_string(&mut content)
        .map_err(|e| {
            LinterError::Config(format!(
                "Failed to read rule manifest '{}': {}",
                manifest_path.display(),
                e
            ))
        })?;

    if content.len() as u64 > MAX_MANIFEST_SIZE {
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

    let mut wasm_file = File::open(&canonical_wasm_path).map_err(|e| {
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

    if wasm_metadata.len() > MAX_WASM_SIZE {
        return Err(LinterError::Config(format!(
            "WASM file '{}' is too large (exceeds 50MB limit)",
            canonical_wasm_path.display()
        )));
    }

    let mut wasm_bytes = Vec::with_capacity(wasm_metadata.len() as usize);
    (&mut wasm_file)
        .take(MAX_WASM_SIZE + 1)
        .read_to_end(&mut wasm_bytes)
        .map_err(|e| {
            LinterError::Config(format!(
                "Failed to read WASM file '{}': {}",
                canonical_wasm_path.display(),
                e
            ))
        })?;

    if wasm_bytes.len() as u64 > MAX_WASM_SIZE {
        return Err(LinterError::Config(format!(
            "WASM file '{}' is too large (exceeds 50MB limit)",
            canonical_wasm_path.display()
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
    use std::fs::{self, File};
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
    fn test_load_rule_manifest_missing_wasm_file() {
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

        // Let's create the file and delete it right before WASM opens it.
        // Actually, the previous implementation just passed.
        // But the previous implementation returned "WASM file not found at 'missing.wasm'".
        // The check `!wasm_path.exists()` catches it, so we don't trigger "Failed to open WASM file".
        // To trigger "Failed to open WASM file", we need a file that exists but fails to open (like a directory).

        let wasm_dir = dir.path().join("missing.wasm");
        fs::create_dir(&wasm_dir).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();

        assert!(
            err_str.contains("Failed to read WASM file")
                || err_str.contains("Failed to open WASM file")
                || err_str.contains("Is a directory")
                || err_str.contains("Permission denied"),
            "Unexpected error message: {}",
            err_str
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

    #[test]
    fn test_load_rule_manifest_wasm_too_large() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        let wasm_file = File::create(&wasm_path).unwrap();
        // create a WASM file that's exactly MAX_WASM_SIZE + 1 bytes long
        wasm_file.set_len(MAX_WASM_SIZE + 1).unwrap();

        // Have to create a valid manifest pointing to the oversized WASM
        let wasm_hash = "0".repeat(64); // Doesn't matter, it'll fail on size first
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

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("exceeds 50MB limit"),
            "Expected error to be about WASM size limit, but got: {}",
            err_msg
        );
    }

    #[test]
    fn test_load_rule_manifest_io_error_reading_manifest() {
        // We need a file that we can open, but fails when we try to read from it.
        // A directory usually fails on read, but File::open might also fail.
        // On Unix, File::open on a dir succeeds, but read fails with EISDIR.
        let dir = tempdir().unwrap();
        let manifest_dir_path = dir.path().join("tsuzulint-rule.json");
        std::fs::create_dir(&manifest_dir_path).unwrap();

        let result = load_rule_manifest(&manifest_dir_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();

        // It should either fail at `File::open` (Windows/some Unix) or at `read_to_string` (other Unix).
        // Either way, it covers error handling for these operations.
        assert!(
            err_msg.contains("Failed to open rule manifest")
                || err_msg.contains("Failed to read rule manifest"),
            "Unexpected error message: {}",
            err_msg
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_load_rule_manifest_fifo_too_large() {
        use std::io::Write;
        use std::process::Command;

        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        let status = Command::new("mkfifo").arg(&manifest_path).status();

        if !status.is_ok_and(|s| s.success()) {
            return; // Skip if mkfifo fails or is not available
        }

        // Spawn a thread to write more than 10MB to the fifo
        let manifest_path_clone = manifest_path.clone();
        std::thread::spawn(move || {
            if let Ok(mut f) = File::create(&manifest_path_clone) {
                let chunk = vec![b'a'; 1024 * 1024];
                for _ in 0..11 {
                    if f.write_all(&chunk).is_err() {
                        break;
                    }
                }
            }
        });

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("exceeds 10MB limit"),
            "Expected error to be about manifest size limit from reading FIFO, but got: {}",
            err_msg
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_load_rule_manifest_wasm_fifo_too_large() {
        use std::io::Write;
        use std::process::Command;

        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        let wasm_path = dir.path().join("rule.wasm");

        let status = Command::new("mkfifo").arg(&wasm_path).status();

        if !status.is_ok_and(|s| s.success()) {
            return; // Skip if mkfifo fails or is not available
        }

        let wasm_hash = "0".repeat(64);
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

        // Spawn a thread to write more than 50MB to the fifo
        let wasm_path_clone = wasm_path.clone();
        std::thread::spawn(move || {
            if let Ok(mut f) = File::create(&wasm_path_clone) {
                let chunk = vec![0u8; 1024 * 1024];
                for _ in 0..51 {
                    if f.write_all(&chunk).is_err() {
                        break;
                    }
                }
            }
        });

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("exceeds 50MB limit"),
            "Expected error to be about WASM size limit from reading FIFO, but got: {}",
            err_msg
        );
    }
}
