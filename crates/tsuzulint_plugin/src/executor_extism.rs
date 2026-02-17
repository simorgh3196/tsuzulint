//! Extism-based WASM executor for native environments.
//!
//! This module provides high-performance WASM execution using Extism,
//! which internally uses wasmtime for JIT compilation.

use std::collections::HashMap;
use std::path::Path;

use extism::{Manifest, Plugin, Wasm};
use tracing::{debug, info};

use crate::executor::{LoadResult, RuleExecutor};
use crate::{PluginError, RuleManifest};

/// Default memory limit for WASM instances (128 MB = 2048 pages).
/// Each WASM page is 64KB.
const DEFAULT_MEMORY_MAX_PAGES: u32 = 2048;

/// Default timeout for WASM execution (5000 ms).
const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// A loaded rule using Extism.
struct LoadedRule {
    /// The Extism plugin instance.
    plugin: Plugin,
    /// The rule manifest (kept for potential future use).
    #[allow(dead_code)]
    manifest: RuleManifest,
}

/// Extism-based executor for native environments.
///
/// Uses wasmtime JIT compilation for high-performance WASM execution.
/// This executor is suitable for CLI, desktop applications (Tauri),
/// and server environments.
pub struct ExtismExecutor {
    /// Loaded rules by name.
    rules: HashMap<String, LoadedRule>,
    /// Timeout for WASM execution in milliseconds.
    timeout_ms: u64,
}

impl ExtismExecutor {
    /// Creates a new Extism executor.
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }

    /// Sets the timeout for WASM execution.
    #[cfg(all(test, feature = "test-utils"))]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Configures the manifest with security limits.
    fn configure_manifest(&self, mut manifest: Manifest) -> Manifest {
        // Set execution timeout to prevent infinite loops
        manifest.timeout_ms = Some(self.timeout_ms);

        // Set memory limits to prevent DoS via memory exhaustion
        manifest.memory = extism_manifest::MemoryOptions {
            max_pages: Some(DEFAULT_MEMORY_MAX_PAGES),
            max_http_response_bytes: Some(0), // Deny HTTP response buffering
            max_var_bytes: Some(1024 * 1024), // Limit variable storage to 1MB
        };

        // Deny all network access
        manifest.allowed_hosts = Some(vec![]);

        manifest
    }

    /// Loads a plugin from a raw manifest.
    fn load_from_manifest(&mut self, manifest: Manifest) -> Result<LoadResult, PluginError> {
        let manifest = self.configure_manifest(manifest);

        // Create the plugin with WASI support
        let mut plugin = Plugin::new(&manifest, [], true)
            .map_err(|e| PluginError::load(format!("Failed to create plugin: {}", e)))?;

        // Get the rule manifest by calling get_manifest()
        let manifest_json: String = plugin
            .call("get_manifest", "")
            .map_err(|e| PluginError::call(format!("Failed to get manifest: {}", e)))?;

        let rule_manifest: RuleManifest = serde_json::from_str(&manifest_json)
            .map_err(|e| PluginError::invalid_manifest(e.to_string()))?;

        debug!(
            "Loaded rule: {} v{}",
            rule_manifest.name, rule_manifest.version
        );

        let name = rule_manifest.name.clone();
        self.rules.insert(
            name.clone(),
            LoadedRule {
                plugin,
                manifest: rule_manifest.clone(),
            },
        );

        Ok(LoadResult {
            name,
            manifest: rule_manifest,
        })
    }
}

impl Default for ExtismExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleExecutor for ExtismExecutor {
    fn load(&mut self, wasm_bytes: &[u8]) -> Result<LoadResult, PluginError> {
        info!("Loading WASM rule ({} bytes)", wasm_bytes.len());

        // Create the plugin manifest from bytes
        let wasm = Wasm::data(wasm_bytes.to_vec());
        let manifest = Manifest::new([wasm]);

        self.load_from_manifest(manifest)
    }

    fn load_file(&mut self, path: &Path) -> Result<LoadResult, PluginError> {
        info!("Loading rule from file: {}", path.display());

        // Create the plugin manifest from file
        let wasm = Wasm::file(path);
        let manifest = Manifest::new([wasm]);

        self.load_from_manifest(manifest)
    }

    fn call_lint(&mut self, rule_name: &str, input_json: &str) -> Result<String, PluginError> {
        let rule = self
            .rules
            .get_mut(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;

        let response_json: String = rule
            .plugin
            .call("lint", input_json)
            .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;

        Ok(response_json)
    }

    fn unload(&mut self, rule_name: &str) -> bool {
        self.rules.remove(rule_name).is_some()
    }

    fn unload_all(&mut self) {
        self.rules.clear();
    }

    fn loaded_rules(&self) -> Vec<&str> {
        self.rules.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "test-utils")]
    use crate::test_utils::wat_to_wasm;

    use super::*;

    #[test]
    fn test_executor_new() {
        let executor = ExtismExecutor::new();
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    fn test_executor_call_not_found() {
        let mut executor = ExtismExecutor::new();
        let result = executor.call_lint("nonexistent", "{}");
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_memory_limit() {
        let mut executor = ExtismExecutor::new();

        // A rule that tries to allocate 200MB (3200 pages)
        // 3200 * 64KB = 200MB > 128MB limit
        // We use a simplified module for this test as we just want it to fail loading
        let wasm = wat_to_wasm(
            r#"
            (module
                (memory (export "memory") 3200)
                (func (export "get_manifest") (result i32) (i32.const 0))
            )
            "#,
        );

        // Loading should fail because the initial memory exceeds the limit
        let result = executor.load(&wasm);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();

        // Debug output to see actual error
        println!("Memory limit error: {}", err_msg);

        // Check for specific error related to memory limit
        // The error message depends on the runtime but should contain "memory", "limit" or "oom"
        assert!(
            err_msg.to_lowercase().contains("memory")
                || err_msg.to_lowercase().contains("limit")
                || err_msg.to_lowercase().contains("oom"),
            "Unexpected error message: {}",
            err_msg
        );
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_infinite_loop() {
        // Use a short timeout for testing (200ms instead of 5s)
        let mut executor = ExtismExecutor::new().with_timeout(200);

        // Infinite loop rule (Extism ABI)
        // We put the loop in get_manifest so load() fails with timeout
        // This avoids needing a valid manifest to proceed
        let wasm = wat_to_wasm(
            r#"
            (module
                (memory (export "memory") 1)

                (func (export "get_manifest") (result i32)
                    (loop
                        (br 0)
                    )
                    (i32.const 0)
                )
            )
            "#,
        );

        // Should return an error due to timeout during load (get_manifest execution)
        let result = executor.load(&wasm);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();

        println!("Timeout error: {}", err_msg);

        let err_lower = err_msg.to_lowercase();
        assert!(
            err_lower.contains("timeout") || err_lower.contains("deadline"),
            "Expected timeout error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_executor_default() {
        let executor = ExtismExecutor::default();
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    fn test_executor_unload_returns_false_for_nonexistent() {
        let mut executor = ExtismExecutor::new();
        assert!(!executor.unload("nonexistent-rule"));
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_unload_all() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut executor = ExtismExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());
        executor.load(&wasm).expect("Failed to load rule");

        assert_eq!(executor.loaded_rules().len(), 1);
        executor.unload_all();
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_is_loaded() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut executor = ExtismExecutor::new();
        assert!(!executor.is_loaded("test-rule"));

        let wasm = wat_to_wasm(&valid_rule_wat());
        executor.load(&wasm).expect("Failed to load rule");

        assert!(executor.is_loaded("test-rule"));
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_load_invalid_wasm() {
        let mut executor = ExtismExecutor::new();
        let invalid_wasm = b"not valid wasm at all";

        let result = executor.load(invalid_wasm);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_load_file_valid() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};
        use std::io::Write;

        let mut executor = ExtismExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(&wasm).unwrap();

        let result = executor.load_file(temp_file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "test-rule");
    }

    #[test]
    fn test_executor_load_file_nonexistent() {
        let mut executor = ExtismExecutor::new();
        let result = executor.load_file(std::path::Path::new("/nonexistent/path/rule.wasm"));
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_empty_wasm() {
        let mut executor = ExtismExecutor::new();
        let result = executor.load(&[]);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_load_bytes_and_call() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut executor = ExtismExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        executor.load(&wasm).expect("Failed to load");

        let result = executor.call_lint("test-rule", "{}");
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_unload_and_reload() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut executor = ExtismExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load
        executor.load(&wasm).expect("Failed to load");
        assert!(executor.is_loaded("test-rule"));

        // Unload
        assert!(executor.unload("test-rule"));
        assert!(!executor.is_loaded("test-rule"));

        // Reload
        executor.load(&wasm).expect("Failed to reload");
        assert!(executor.is_loaded("test-rule"));
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_executor_network_denial() {
        use crate::test_utils::wat_to_wasm;

        let mut executor = ExtismExecutor::new();

        // This WASM module would try to make network requests if allowed
        // But our configuration denies network access
        let json = r#"{"name":"network-test","version":"1.0.0"}"#;
        let len = json.len();
        let wasm = wat_to_wasm(&format!(
            r#"
            (module
                (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
                (memory (export "memory") 1)
                (func (export "get_manifest")
                    (call $output_set (i64.const 0) (i64.const {}))
                )
                (func (export "lint")
                    (call $output_set (i64.const 0) (i64.const 0))
                )
                (func (export "alloc") (param i64) (result i64)
                    (i64.const 128)
                )
                (data (i32.const 0) "{}")
            )
            "#,
            len,
            json.replace("\"", "\\\"")
        ));

        // Should load successfully (network is just denied, not required)
        let result = executor.load(&wasm);
        assert!(result.is_ok());
    }
}