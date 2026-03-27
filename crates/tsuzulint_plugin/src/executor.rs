//! Rule executor abstraction.
//!
//! This module provides the `RuleExecutor` trait which abstracts
//! the WASM runtime implementation, allowing different backends
//! for native (Extism) and browser (wasmi) environments.

use crate::{PluginError, RuleManifest};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Options for configuring a WASM plugin payload at load time.
#[derive(Debug, Clone, Default)]
pub struct PluginOptions {
    /// Allowed hosts for network requests. If `None`, all are denied.
    pub allowed_hosts: Option<Vec<String>>,
    /// Allowed local filesystem paths. Map of alias to actual path.
    pub allowed_paths: Option<BTreeMap<String, PathBuf>>,
    /// Initial configuration variables for the plugin.
    pub config: BTreeMap<String, String>,
    /// Limit on memory pages (each page is 64KB).
    pub memory_max_pages: Option<u32>,
    /// Limit on HTTP response bytes.
    pub memory_max_http_response_bytes: Option<u64>,
    /// Execution timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Wasmtime compilation cache configuration file path (TOML). If `None`, caching is disabled or uses default.
    pub wasmtime_cache_config_path: Option<PathBuf>,
}

/// Result from loading a WASM rule.
#[derive(Debug)]
pub struct LoadResult {
    /// The rule name extracted from the manifest.
    pub name: String,
    /// The rule manifest.
    pub manifest: RuleManifest,
}

/// Reads a WASM file from disk and validates its size limits.
pub fn read_wasm_file_or_error(path: &std::path::Path) -> Result<Vec<u8>, crate::PluginError> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| crate::PluginError::load(format!("Failed to open file: {}", e)))?;

    let metadata = file
        .metadata()
        .map_err(|e| crate::PluginError::load(format!("Failed to read file metadata: {}", e)))?;

    if metadata.len() > crate::MAX_WASM_SIZE {
        return Err(crate::PluginError::load(format!(
            "WASM file size {} exceeds maximum allowed size of {} bytes",
            metadata.len(),
            crate::MAX_WASM_SIZE
        )));
    }

    let mut wasm_bytes: Vec<u8> = Vec::new();
    let bytes_read = (&mut file)
        .take(crate::MAX_WASM_SIZE + 1)
        .read_to_end(&mut wasm_bytes)
        .map_err(|e| crate::PluginError::load(format!("Failed to read file: {}", e)))?;

    if bytes_read > crate::MAX_WASM_SIZE as usize {
        return Err(crate::PluginError::load(format!(
            "WASM file size exceeds maximum allowed size of {} bytes",
            crate::MAX_WASM_SIZE
        )));
    }

    Ok(wasm_bytes)
}

/// Trait for WASM rule execution.
///
/// This trait abstracts the underlying WASM runtime, allowing
/// different implementations for different environments:
///
/// - `ExtismExecutor`: High-performance JIT execution for native environments
/// - `WasmiExecutor`: Pure Rust interpreter for browser/WASM environments
pub trait RuleExecutor {
    /// Loads a WASM rule from bytes.
    ///
    /// # Arguments
    ///
    /// * `wasm_bytes` - The WASM binary content
    /// * `options` - Plugin execution options
    ///
    /// # Returns
    ///
    /// The rule name and manifest on success.
    fn load(
        &mut self,
        wasm_bytes: &[u8],
        options: PluginOptions,
    ) -> Result<LoadResult, PluginError>;

    /// Loads a WASM rule from a file path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the WASM file
    /// * `options` - Plugin execution options
    ///
    /// # Returns
    ///
    /// The rule name and manifest on success.
    fn load_file(
        &mut self,
        path: &std::path::Path,
        options: PluginOptions,
    ) -> Result<LoadResult, PluginError> {
        let wasm_bytes = read_wasm_file_or_error(path)?;

        self.load(&wasm_bytes, options)
    }

    /// Configures a loaded rule.
    ///
    /// # Arguments
    ///
    /// * `rule_name` - Name of the rule to configure
    /// * `config` - The configuration object
    fn configure(&mut self, rule_name: &str, config: &serde_json::Value)
    -> Result<(), PluginError>;

    /// Calls the `lint` function of a loaded rule.
    ///
    /// # Arguments
    ///
    /// * `rule_name` - Name of the rule to call
    /// * `input_bytes` - Msgpack-serialized LintRequest
    ///
    /// # Returns
    ///
    /// Msgpack-serialized LintResponse on success.
    fn call_lint(&mut self, rule_name: &str, input_bytes: &[u8]) -> Result<Vec<u8>, PluginError>;

    /// Unloads a rule.
    ///
    /// # Arguments
    ///
    /// * `rule_name` - Name of the rule to unload
    ///
    /// # Returns
    ///
    /// `true` if the rule was unloaded, `false` if it wasn't loaded.
    fn unload(&mut self, rule_name: &str) -> bool;

    /// Unloads all rules.
    fn unload_all(&mut self);

    /// Returns the names of all loaded rules.
    fn loaded_rules(&self) -> Vec<&str>;

    /// Checks if a rule is loaded.
    fn is_loaded(&self, rule_name: &str) -> bool {
        self.loaded_rules().contains(&rule_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::NamedTempFile;

    struct DummyExecutor;

    impl RuleExecutor for DummyExecutor {
        fn load(
            &mut self,
            _wasm_bytes: &[u8],
            _options: PluginOptions,
        ) -> Result<LoadResult, PluginError> {
            Ok(LoadResult {
                name: "dummy".to_string(),
                manifest: RuleManifest {
                    name: "dummy".to_string(),
                    version: "1.0.0".to_string(),
                    description: None,
                    fixable: false,
                    node_types: vec![],
                    isolation_level: crate::IsolationLevel::default(),
                    capabilities: vec![],
                    schema: None,
                    languages: vec![],
                },
            })
        }
        fn configure(
            &mut self,
            _rule_name: &str,
            _config: &serde_json::Value,
        ) -> Result<(), PluginError> {
            Ok(())
        }
        fn call_lint(
            &mut self,
            _rule_name: &str,
            _input_bytes: &[u8],
        ) -> Result<Vec<u8>, PluginError> {
            Ok(vec![])
        }
        fn unload(&mut self, _rule_name: &str) -> bool {
            true
        }
        fn unload_all(&mut self) {}
        fn loaded_rules(&self) -> Vec<&str> {
            vec![]
        }
    }

    #[test]
    fn test_load_file_not_found() {
        let mut executor = DummyExecutor;
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let missing_path = temp_dir.path().join("definitely-missing-rule.wasm");
        let result = executor.load_file(&missing_path, PluginOptions::default());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to open file")
        );
    }

    #[test]
    fn test_load_file_exceeds_size() {
        let mut executor = DummyExecutor;
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        let f = File::create(path).unwrap();
        f.set_len(crate::MAX_WASM_SIZE + 10).unwrap();

        let result = executor.load_file(path, PluginOptions::default());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds maximum allowed size")
        );
    }

    #[test]
    fn test_load_file_success() {
        let mut executor = DummyExecutor;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"dummy data").unwrap();
        file.flush().unwrap();

        let result = executor.load_file(file.path(), PluginOptions::default());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "dummy");
    }
}
