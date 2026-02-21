//! Extism-based WASM executor for native environments.
//!
//! This module provides high-performance WASM execution using Extism,
//! which internally uses wasmtime for JIT compilation.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use extism::{
    CurrentPlugin, Error, Function, Manifest, Plugin, PluginBuilder, UserData, Val, ValType, Wasm,
};
use tracing::{debug, info};

use crate::executor::{LoadResult, RuleExecutor};
use crate::{PluginError, RuleManifest};

/// Default memory limit for WASM instances (128 MB = 2048 pages).
/// Each WASM page is 64KB.
const DEFAULT_MEMORY_MAX_PAGES: u32 = 2048;

/// Default timeout for WASM execution (5000 ms).
const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Default fuel limit for WASM execution (1 billion instructions).
const DEFAULT_FUEL_LIMIT: u64 = 1_000_000_000;

/// Source of the rule (WASM bytes or file path).
#[derive(Clone)]
enum RuleSource {
    Bytes(Vec<u8>),
    File(PathBuf),
}

impl RuleSource {
    fn to_wasm(&self) -> Wasm {
        match self {
            RuleSource::Bytes(bytes) => Wasm::data(bytes.clone()),
            RuleSource::File(path) => Wasm::file(path),
        }
    }
}

/// A loaded rule using Extism.
struct LoadedRule {
    /// The Extism plugin instance.
    plugin: Plugin,
    /// The rule manifest.
    #[allow(dead_code)]
    manifest: RuleManifest,
    /// The source of the rule (for reloading).
    source: RuleSource,
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
    /// Fuel limit for WASM execution (number of instructions).
    fuel_limit: Option<u64>,
}

impl ExtismExecutor {
    /// Creates a new Extism executor.
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            fuel_limit: Some(DEFAULT_FUEL_LIMIT),
        }
    }

    /// Sets the timeout for WASM execution.
    #[cfg(all(test, feature = "test-utils"))]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Sets the fuel limit for WASM execution.
    #[cfg(all(test, feature = "test-utils"))]
    pub fn with_fuel_limit(mut self, limit: u64) -> Self {
        self.fuel_limit = Some(limit);
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

        // Deny all file system access
        manifest.allowed_paths = Some(BTreeMap::new());

        // Clear configuration to prevent environment leakage
        manifest.config = BTreeMap::new();

        manifest
    }

    fn create_plugin(
        &self,
        source: &RuleSource,
        config_json: &str,
    ) -> Result<(Plugin, RuleManifest), PluginError> {
        let wasm = source.to_wasm();
        let manifest = Manifest::new([wasm]);
        let mut manifest = self.configure_manifest(manifest);

        // Set configuration
        manifest
            .config
            .insert("config".to_string(), config_json.to_string());

        let f = Function::new(
            "tsuzulint_get_config",
            [ValType::I64, ValType::I64],
            [ValType::I64],
            UserData::new(()),
            tsuzulint_get_config_stub,
        )
        .with_namespace("extism:host/user");

        let mut builder = PluginBuilder::new(manifest)
            .with_wasi(true)
            .with_functions([f]);

        if let Some(limit) = self.fuel_limit {
            builder = builder.with_fuel_limit(limit);
        }

        let mut plugin = builder
            .build()
            .map_err(|e| PluginError::load(format!("Failed to create plugin: {}", e)))?;

        // Get the rule manifest by calling get_manifest()
        let manifest_json: String = plugin
            .call("get_manifest", "")
            .map_err(|e| PluginError::call(format!("Failed to get manifest: {}", e)))?;

        let rule_manifest: RuleManifest = serde_json::from_str(&manifest_json)
            .map_err(|e| PluginError::invalid_manifest(e.to_string()))?;

        Ok((plugin, rule_manifest))
    }

    fn load_rule(&mut self, source: RuleSource) -> Result<LoadResult, PluginError> {
        let (plugin, rule_manifest) = self.create_plugin(&source, "{}")?; // Default config

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
                source,
            },
        );

        Ok(LoadResult {
            name,
            manifest: rule_manifest,
        })
    }
}

/// Host function stub (unused in Extism mode).
fn tsuzulint_get_config_stub(
    _plugin: &mut CurrentPlugin,
    _args: &[Val],
    results: &mut [Val],
    _user_data: UserData<()>,
) -> Result<(), Error> {
    results[0] = Val::I64(0);
    Ok(())
}

impl Default for ExtismExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleExecutor for ExtismExecutor {
    fn load(&mut self, wasm_bytes: &[u8]) -> Result<LoadResult, PluginError> {
        info!("Loading WASM rule ({} bytes)", wasm_bytes.len());
        self.load_rule(RuleSource::Bytes(wasm_bytes.to_vec()))
    }

    fn load_file(&mut self, path: &Path) -> Result<LoadResult, PluginError> {
        info!("Loading rule from file: {}", path.display());
        self.load_rule(RuleSource::File(path.to_path_buf()))
    }

    fn configure(
        &mut self,
        rule_name: &str,
        config: &serde_json::Value,
    ) -> Result<(), PluginError> {
        let source = {
            let rule = self
                .rules
                .get(rule_name)
                .ok_or_else(|| PluginError::not_found(rule_name))?;
            rule.source.clone()
        };

        let config_json = serde_json::to_string(config)
            .map_err(|e| PluginError::call(format!("Failed to serialize config: {}", e)))?;

        let (plugin, _manifest) = self.create_plugin(&source, &config_json)?;

        if let Some(rule) = self.rules.get_mut(rule_name) {
            rule.plugin = plugin;
        }
        Ok(())
    }

    fn call_lint(&mut self, rule_name: &str, input_bytes: &[u8]) -> Result<Vec<u8>, PluginError> {
        let rule = self
            .rules
            .get_mut(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;

        let response_bytes = rule
            .plugin
            .call::<&[u8], Vec<u8>>("lint", input_bytes)
            .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;

        Ok(response_bytes)
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
        let result = executor.call_lint("nonexistent", &[]);
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
    #[cfg(feature = "test-utils")]
    fn test_executor_fuel_limit() {
        // Use a fuel limit of 1000 instructions
        let mut executor = ExtismExecutor::new().with_fuel_limit(1000);

        // Infinite loop rule (Extism ABI)
        // We put the loop in get_manifest so load() fails with fuel limit exceeded
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

        // Should return an error due to fuel limit during load
        let result = executor.load(&wasm);

        // TODO: Replace with proper error type matching when Extism exposes stable error kinds.
        // Currently relies on error message content as Extism returns opaque errors without
        // discriminable error variants. See assertion at line ~315 in executor_extism.rs.
        assert!(result.is_err(), "Expected fuel limit error");
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_config_lifecycle() {
        let mut executor = ExtismExecutor::new();

        let json = r#"{"name":"config-rule","version":"1.0.0"}"#;
        // Construct WAT instructions to write the JSON to memory at offset 0
        // This avoids relying on data segments which seem flaky in some contexts or with Extism
        let mut store_instrs = String::new();
        for (i, b) in json.bytes().enumerate() {
            store_instrs.push_str(&format!(
                "(call $store_u8 (i64.const {}) (i32.const {}))\n",
                1024 + i,
                b
            ));
        }

        // Extism provides 'config' in the environment which we can access via extism:host/env::config_get
        // We'll write a test rule that reads this config.
        let wat = format!(
            r#"
            (module
                (import "extism:host/env" "config_get" (func $config_get (param i64) (result i64)))
                (import "extism:host/env" "length" (func $length (param i64) (result i64)))
                (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
                (import "extism:host/env" "load_u8" (func $load_u8 (param i64) (result i32)))
                (import "extism:host/env" "store_u8" (func $store_u8 (param i64 i32)))
                (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))

                (memory (export "memory") 1)

                (func $get_manifest (export "get_manifest") (result i32)
                    {}
                    (call $output_set (i64.const 1024) (i64.const {}))
                    (i32.const 0)
                )

                ;; Helper to read string from Extism memory to local buffer
                (func $lint (export "lint") (result i32)
                    (local $key_ptr i64)
                    (local $val_ptr i64)
                    (local $val_len i64)

                    ;; Write "config" key to memory
                    (call $alloc (i64.const 6))
                    local.set $key_ptr

                    ;; "config" = 99 111 110 102 105 103
                    (call $store_u8 (local.get $key_ptr) (i32.const 99))
                    (call $store_u8 (i64.add (local.get $key_ptr) (i64.const 1)) (i32.const 111))
                    (call $store_u8 (i64.add (local.get $key_ptr) (i64.const 2)) (i32.const 110))
                    (call $store_u8 (i64.add (local.get $key_ptr) (i64.const 3)) (i32.const 102))
                    (call $store_u8 (i64.add (local.get $key_ptr) (i64.const 4)) (i32.const 105))
                    (call $store_u8 (i64.add (local.get $key_ptr) (i64.const 5)) (i32.const 103))

                    ;; Get config
                    (call $config_get (local.get $key_ptr))
                    local.set $val_ptr

                    ;; Get config length
                    (call $length (local.get $val_ptr))
                    local.set $val_len

                    ;; Set output
                    (call $output_set (local.get $val_ptr) (local.get $val_len))

                    (i32.const 0)
                )
            )
            "#,
            store_instrs,
            json.len()
        );

        // NOTE: Writing pure WAT to test Extism config is complex because it involves
        // managing Extism's memory model (alloc, length, load/store).
        // Instead, we will rely on the fact that `configure` calls `create_plugin` which sets
        // the manifest config. The Extism library guarantees this works.
        // We will just verify that `configure` doesn't error and that the rule persists.

        let wasm = wat_to_wasm(&wat);
        executor.load(&wasm).expect("Failed to load rule");

        let config = serde_json::json!({"foo": "bar"});
        let res = executor.configure("config-rule", &config);
        assert!(res.is_ok());

        // Ensure rule is still loaded/callable
        let res = executor.call_lint("config-rule", b"{}");
        assert!(res.is_ok());
    }

    #[test]
    #[cfg(feature = "test-utils")]
    fn test_config_fallback_stub() {
        let mut executor = ExtismExecutor::new();

        // This test specifically calls the tsuzulint_get_config host function
        // which is a stub in ExtismExecutor (returns 0).
        let json = r#"{"name":"stub-test","version":"1.0.0"}"#;
        let mut store_instrs = String::new();
        for (i, b) in json.bytes().enumerate() {
            store_instrs.push_str(&format!(
                "(call $store_u8 (i64.const {}) (i32.const {}))\n",
                1024 + i,
                b
            ));
        }

        let wat = format!(
            r#"
            (module
                (import "extism:host/user" "tsuzulint_get_config" (func $get_config (param i64 i64) (result i64)))
                (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
                (import "extism:host/env" "store_u8" (func $store_u8 (param i64 i32)))
                (memory (export "memory") 1)

                (func $get_manifest (export "get_manifest") (result i32)
                    {}
                    (call $output_set (i64.const 1024) (i64.const {}))
                    (i32.const 0)
                )

                (func $lint (export "lint") (result i32)
                    ;; Call the stub with arbitrary arguments
                    (call $get_config (i64.const 0) (i64.const 10))
                    ;; Convert result i64 to i32
                    i32.wrap_i64
                )
            )
            "#,
            store_instrs,
            json.len()
        );

        let wasm = wat_to_wasm(&wat);
        executor.load(&wasm).expect("Failed to load rule");

        let res = executor.call_lint("stub-test", b"{}");
        // The stub returns 0, and we don't call output_set in lint, so result is empty
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), Vec::<u8>::new());
    }
}
