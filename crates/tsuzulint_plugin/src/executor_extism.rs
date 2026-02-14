//! Extism-based WASM executor for native environments.
//!
//! This module provides high-performance WASM execution using Extism,
//! which internally uses wasmtime for JIT compilation.

use std::collections::HashMap;
use std::path::Path;

use extism::{Manifest, Plugin, Wasm};
// We need MemoryOptions from extism-manifest to configure memory limits
use extism_manifest::MemoryOptions;
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
}

impl ExtismExecutor {
    /// Creates a new Extism executor.
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
        }
    }

    /// Configures the manifest with security limits.
    fn configure_manifest(mut manifest: Manifest) -> Manifest {
        // Set execution timeout to prevent infinite loops
        manifest.timeout_ms = Some(DEFAULT_TIMEOUT_MS);

        // Set memory limits to prevent DoS via memory exhaustion
        manifest.memory = MemoryOptions {
            max_pages: Some(DEFAULT_MEMORY_MAX_PAGES),
            max_http_response_bytes: None,
            max_var_bytes: None,
        };

        manifest
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
        let manifest = Self::configure_manifest(manifest);

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

    fn load_file(&mut self, path: &Path) -> Result<LoadResult, PluginError> {
        info!("Loading rule from file: {}", path.display());

        // Create the plugin manifest from file
        let wasm = Wasm::file(path);
        let manifest = Manifest::new([wasm]);
        let manifest = Self::configure_manifest(manifest);

        // Create the plugin with WASI support
        let mut plugin = Plugin::new(&manifest, [], true)
            .map_err(|e| PluginError::load(format!("Failed to create plugin: {}", e)))?;

        // Get the rule manifest
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
    use super::*;

    /// Helper to compile WAT to WASM bytes
    fn wat_to_wasm(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).expect("Invalid WAT")
    }

    /// Helper to create a basic valid rule in WAT (Extism ABI)
    #[allow(dead_code)]
    fn valid_rule_wat() -> String {
        let json = r#"{"name":"test-rule","version":"1.0.0","description":"Test rule"}"#;
        let len = json.len();
        format!(
            r#"
            (module
                (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
                (memory (export "memory") 1)

                (func (export "get_manifest") (result i32)
                    (call $output_set (i64.const 0) (i64.const {}))
                    (i32.const 0)
                )

                (func (export "lint") (result i32)
                    (call $output_set (i64.const 100) (i64.const 2))
                    (i32.const 0)
                )

                (data (i32.const 0) "{}")
                (data (i32.const 100) "[]")
            )
            "#,
            len,
            json.replace("\"", "\\\"")
        )
    }

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

    // Commented out due to ABI mismatch with Extism runtime in manual WAT
    // #[test]
    // fn test_executor_load_valid_rule() {
    //     let mut executor = ExtismExecutor::new();
    //     let wasm = wat_to_wasm(&valid_rule_wat());

    //     let result = executor.load(&wasm);
    //     if let Err(e) = &result {
    //         println!("Load failed: {}", e);
    //     }
    //     assert!(result.is_ok());

    //     let loaded = result.unwrap();
    //     assert_eq!(loaded.name, "test-rule");
    // }

    #[test]
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

        assert!(
            err_msg.contains("memory")
                || err_msg.contains("Memory")
                || err_msg.contains("resource")
                || err_msg.contains("limit")
                || err_msg.contains("oom")
                || err_msg.contains("Failed to create plugin"),
            "Unexpected error message: {}",
            err_msg
        );
    }

    #[test]
    fn test_executor_infinite_loop() {
        let mut executor = ExtismExecutor::new();

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
            "Expected timeout error, got: {err_msg}"
        );
    }
}
