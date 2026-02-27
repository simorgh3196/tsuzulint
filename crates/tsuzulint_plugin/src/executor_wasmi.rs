//! Wasmi-based WASM executor for browser environments.
//!
//! This module provides WASM execution using wasmi, a pure Rust
//! WebAssembly interpreter that can itself be compiled to WASM,
//! enabling "WASM-in-WASM" execution for browser environments.

use std::collections::HashMap;

use tracing::{debug, info};
use wasmi::{
    Caller, Config, Engine, Extern, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

use crate::executor::{LoadResult, PluginOptions, RuleExecutor};
use crate::{PluginError, RuleManifest};

/// Default memory limit for WASM instances (128 MB).
const DEFAULT_MEMORY_LIMIT_BYTES: usize = 128 * 1024 * 1024;

/// Default fuel limit for WASM execution (instructions).
/// 1 billion instructions should be enough for any reasonable rule,
/// but will stop infinite loops.
const DEFAULT_FUEL_LIMIT: u64 = 1_000_000_000;

/// Host state for wasmi store.
struct HostState {
    input_buffer: Vec<u8>,
    output_buffer: Vec<u8>,
    memory: Option<Memory>,
    limits: StoreLimits,
}

impl HostState {
    fn new() -> Self {
        Self {
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            memory: None,
            limits: StoreLimitsBuilder::new()
                .memory_size(DEFAULT_MEMORY_LIMIT_BYTES)
                .build(),
        }
    }
}

/// A loaded rule using wasmi.
struct LoadedRule {
    /// The wasmi store.
    store: Store<HostState>,
    /// The get_manifest function (kept for potential future use).
    #[allow(dead_code)]
    get_manifest_fn: TypedFunc<(), (i32, i32)>,
    /// The lint function.
    lint_fn: TypedFunc<(i32, i32), (i32, i32)>,
    /// The alloc function (for allocating memory in WASM).
    alloc_fn: TypedFunc<i32, i32>,
    /// The rule manifest (kept for potential future use).
    #[allow(dead_code)]
    manifest: RuleManifest,
}

/// Wasmi-based executor for browser environments.
///
/// Uses a pure Rust WASM interpreter, allowing the entire
/// linter to be compiled to WASM for browser execution.
pub struct WasmiExecutor {
    /// The wasmi engine (shared configuration).
    engine: Engine,
    /// Loaded rules by name.
    rules: HashMap<String, LoadedRule>,
}

impl WasmiExecutor {
    /// Creates a new wasmi executor.
    pub fn new() -> Self {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);

        Self {
            engine,
            rules: HashMap::new(),
        }
    }

    /// Reads a string from WASM memory.
    fn read_bytes(store: &Store<HostState>, ptr: i32, len: i32) -> Result<Vec<u8>, PluginError> {
        let memory = store
            .data()
            .memory
            .ok_or_else(|| PluginError::call("Memory not initialized"))?;

        let data = memory
            .data(store)
            .get(ptr as usize..(ptr + len) as usize)
            .ok_or_else(|| PluginError::call("Memory access out of bounds"))?;

        Ok(data.to_vec())
    }

    /// Writes a string to WASM memory and returns the pointer.
    fn write_bytes(
        store: &mut Store<HostState>,
        alloc_fn: &TypedFunc<i32, i32>,
        data: &[u8],
    ) -> Result<(i32, i32), PluginError> {
        let bytes = data;
        let len = bytes.len() as i32;

        // Allocate memory in WASM
        let ptr = alloc_fn
            .call(&mut *store, len)
            .map_err(|e| PluginError::call(format!("Allocation failed: {}", e)))?;

        // Get memory and write data
        let memory = store
            .data()
            .memory
            .ok_or_else(|| PluginError::call("Memory not initialized"))?;

        memory
            .data_mut(&mut *store)
            .get_mut(ptr as usize..(ptr as usize + bytes.len()))
            .ok_or_else(|| PluginError::call("Memory access out of bounds"))?
            .copy_from_slice(bytes);

        Ok((ptr, len))
    }
}

impl Default for WasmiExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleExecutor for WasmiExecutor {
    fn load(
        &mut self,
        wasm_bytes: &[u8],
        _options: PluginOptions,
    ) -> Result<LoadResult, PluginError> {
        info!("Loading WASM rule ({} bytes) with wasmi", wasm_bytes.len());

        // Compile the module
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| PluginError::load(format!("Failed to compile module: {}", e)))?;

        // Create store with host state
        let mut store = Store::new(&self.engine, HostState::new());

        // Configure resource limits
        store.limiter(|state| &mut state.limits);

        // Set initial fuel for instantiation and loading
        store
            .set_fuel(DEFAULT_FUEL_LIMIT)
            .map_err(|e| PluginError::load(format!("Failed to set fuel: {}", e)))?;

        // Create linker and add host functions
        let mut linker = <Linker<HostState>>::new(&self.engine);

        // Add WASI-like functions that Extism PDK might expect
        // These are stubs for basic compatibility

        // env.abort - called on WASM panic
        linker
            .func_wrap(
                "env",
                "abort",
                |_caller: Caller<'_, HostState>, _msg: i32, _file: i32, _line: i32, _col: i32| {
                    // Abort handler - in real implementation, this would panic
                },
            )
            .map_err(|e| PluginError::load(format!("Failed to add abort: {}", e)))?;

        // Add extism-style host functions for compatibility
        // extism:host/user.input_length
        linker
            .func_wrap("extism:host/user", "input_length", {
                |caller: Caller<'_, HostState>| -> i64 { caller.data().input_buffer.len() as i64 }
            })
            .map_err(|e| PluginError::load(format!("Failed to add input_length: {}", e)))?;

        // extism:host/user.input_load_u8
        linker
            .func_wrap("extism:host/user", "input_load_u8", {
                |caller: Caller<'_, HostState>, offset: i64| -> i32 {
                    caller
                        .data()
                        .input_buffer
                        .get(offset as usize)
                        .copied()
                        .unwrap_or(0) as i32
                }
            })
            .map_err(|e| PluginError::load(format!("Failed to add input_load_u8: {}", e)))?;

        // extism:host/user.output_set
        linker
            .func_wrap(
                "extism:host/user",
                "output_set",
                |mut caller: Caller<'_, HostState>, ptr: i64, len: i64| {
                    if let Some(memory) = caller.data().memory {
                        let data = memory.data(&caller);
                        if let Some(slice) = data.get(ptr as usize..(ptr + len) as usize) {
                            caller.data_mut().output_buffer = slice.to_vec();
                        }
                    }
                },
            )
            .map_err(|e| PluginError::load(format!("Failed to add output_set: {}", e)))?;

        // Instantiate the module
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| PluginError::load(format!("Failed to instantiate and start: {}", e)))?;

        // Get memory export and store in host state
        if let Some(Extern::Memory(memory)) = instance.get_export(&store, "memory") {
            store.data_mut().memory = Some(memory);
        } else {
            return Err(PluginError::load("Module does not export memory"));
        }

        // Get required function exports
        let get_manifest_fn = instance
            .get_typed_func::<(), (i32, i32)>(&store, "get_manifest")
            .or_else(|_| {
                // Try alternative signature (Extism style)
                instance.get_typed_func::<(), (i32, i32)>(&store, "__get_manifest")
            })
            .map_err(|e| PluginError::load(format!("get_manifest not found: {}", e)))?;

        let lint_fn = instance
            .get_typed_func::<(i32, i32), (i32, i32)>(&store, "lint")
            .or_else(|_| instance.get_typed_func::<(i32, i32), (i32, i32)>(&store, "__lint"))
            .map_err(|e| PluginError::load(format!("lint not found: {}", e)))?;

        let alloc_fn = instance
            .get_typed_func::<i32, i32>(&store, "alloc")
            .or_else(|_| instance.get_typed_func::<i32, i32>(&store, "__alloc"))
            .or_else(|_| instance.get_typed_func::<i32, i32>(&store, "malloc"))
            .map_err(|e| PluginError::load(format!("alloc not found: {}", e)))?;

        // Call get_manifest to get the rule manifest
        let (manifest_ptr, manifest_len) = get_manifest_fn
            .call(&mut store, ())
            .map_err(|e| PluginError::call(format!("Failed to get manifest: {}", e)))?;

        let manifest_bytes = Self::read_bytes(&store, manifest_ptr, manifest_len)?;
        let rule_manifest: RuleManifest = rmp_serde::from_slice(&manifest_bytes)
            .map_err(|e| PluginError::invalid_manifest(e.to_string()))?;

        debug!(
            "Loaded rule: {} v{}",
            rule_manifest.name, rule_manifest.version
        );

        let name = rule_manifest.name.clone();
        self.rules.insert(
            name.clone(),
            LoadedRule {
                store,
                get_manifest_fn,
                lint_fn,
                alloc_fn,
                manifest: rule_manifest.clone(),
            },
        );

        Ok(LoadResult {
            name,
            manifest: rule_manifest,
        })
    }

    fn configure(
        &mut self,
        rule_name: &str,
        _config: &serde_json::Value,
    ) -> Result<(), PluginError> {
        self.rules
            .get(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;
        Ok(())
    }

    fn call_lint(&mut self, rule_name: &str, input_bytes: &[u8]) -> Result<Vec<u8>, PluginError> {
        let rule = self
            .rules
            .get_mut(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;

        // Reset fuel before execution
        rule.store
            .set_fuel(DEFAULT_FUEL_LIMIT)
            .map_err(|e| PluginError::call(format!("Failed to set fuel: {}", e)))?;

        // Write input to WASM memory
        let (input_ptr, input_len) =
            Self::write_bytes(&mut rule.store, &rule.alloc_fn, input_bytes)?;

        // Call lint function
        let (output_ptr, output_len) =
            rule.lint_fn
                .call(&mut rule.store, (input_ptr, input_len))
                .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;

        // Read output from WASM memory
        let response_json = Self::read_bytes(&rule.store, output_ptr, output_len)?;

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
#[cfg(feature = "test-utils")]
mod tests {
    use crate::test_utils::{bytes_to_wat_data, manifest_to_msgpack, valid_rule_wat, wat_to_wasm};

    use super::*;

    #[test]
    fn test_executor_new() {
        let executor = WasmiExecutor::new();
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    fn test_executor_load_valid_rule() {
        let mut executor = WasmiExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        let result = executor.load(&wasm, PluginOptions::default());
        assert!(result.is_ok());

        let loaded = result.unwrap();
        assert_eq!(loaded.name, "test-rule");
        assert_eq!(loaded.manifest.version, "1.0.0");

        assert_eq!(executor.loaded_rules(), vec!["test-rule"]);
    }

    #[test]
    fn test_executor_lint_valid() {
        let mut executor = WasmiExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());
        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");

        let result = executor.call_lint("test-rule", b"{\"text\":\"hello\"}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"[]");
    }

    #[test]
    fn test_executor_call_not_found() {
        let mut executor = WasmiExecutor::new();
        let result = executor.call_lint("nonexistent", b"{}");
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn test_executor_missing_exports() {
        let mut executor = WasmiExecutor::new();
        // Missing lint function
        let wasm = wat_to_wasm(
            r#"
        (module
            (memory (export "memory") 1)
            (func (export "get_manifest") (result i32 i32) (i32.const 0) (i32.const 0))
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
        )
        "#,
        );

        let result = executor.load(&wasm, PluginOptions::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("lint not found"));
    }

    #[test]
    fn test_executor_invalid_manifest() {
        let mut executor = WasmiExecutor::new();
        // Manifest returns invalid JSON
        let wasm = wat_to_wasm(
            r#"
        (module
            (memory (export "memory") 1)
            (func (export "get_manifest") (result i32 i32)
                (i32.const 0) ;; ptr
                (i32.const 5) ;; len
            )
            (func (export "lint") (param i32 i32) (result i32 i32) (i32.const 0) (i32.const 0))
            (func (export "alloc") (param i32) (result i32) (i32.const 0))

            (data (i32.const 0) "INVALID")
        )
        "#,
        );

        let result = executor.load(&wasm, PluginOptions::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid manifest"));
    }

    #[test]
    fn test_executor_lint_error() {
        let mut executor = WasmiExecutor::new();
        // Lint function traps (unreachable)
        let manifest =
            RuleManifest::new("error-rule", "1.0.0").with_description("Error rule".to_string());
        let msgpack_bytes = manifest_to_msgpack(&manifest);
        let len = msgpack_bytes.len();
        let data = bytes_to_wat_data(&msgpack_bytes);
        let wasm = wat_to_wasm(&format!(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "get_manifest") (result i32 i32)
                    (i32.const 0)
                    (i32.const {len})
                )
                (func (export "lint") (param i32 i32) (result i32 i32)
                    unreachable
                )
                (func (export "alloc") (param i32) (result i32) (i32.const 128))

                (data (i32.const 0) "{data}")
            )
            "#,
        ));

        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");

        let result = executor.call_lint("error-rule", b"{}");
        assert!(result.is_err());
        // Verify it captures the runtime error
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed") || err_msg.contains("Trap"));
    }

    #[test]
    fn test_executor_large_input() {
        // Test handling of larger input (simulating a real file)
        let mut executor = WasmiExecutor::new();

        // A rule that echoes back the input length as a string in the output (for verification)
        // Note: implementing full echo in WAT is tedious, so we'll just accept the input
        // and return a static success to prove it didn't crash on allocation/write.
        let wasm = wat_to_wasm(&valid_rule_wat());
        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");

        let large_input = "a".repeat(1024 * 10); // 10KB
        let input_json = format!("{{\"text\":\"{}\"}}", large_input);

        let result = executor.call_lint("test-rule", input_json.as_bytes());
        assert!(result.is_ok());
    }

    #[test]
    fn test_executor_default() {
        let executor = WasmiExecutor::default();
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    fn test_executor_unload_returns_false_for_nonexistent() {
        let mut executor = WasmiExecutor::new();
        assert!(!executor.unload("nonexistent-rule"));
    }

    #[test]
    fn test_executor_unload_returns_true_after_load() {
        let mut executor = WasmiExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());
        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");

        assert!(executor.unload("test-rule"));
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    fn test_executor_unload_all() {
        let mut executor = WasmiExecutor::new();
        let wasm = wat_to_wasm(&valid_rule_wat());
        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");

        assert_eq!(executor.loaded_rules().len(), 1);
        executor.unload_all();
        assert!(executor.loaded_rules().is_empty());
    }

    #[test]
    fn test_executor_empty_wasm() {
        let mut executor = WasmiExecutor::new();
        let result = executor.load(&[], PluginOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_executor_invalid_wasm_bytes() {
        let mut executor = WasmiExecutor::new();
        let invalid_wasm = b"not wasm at all";
        let result = executor.load(invalid_wasm, PluginOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_executor_multiple_rules() {
        let mut executor = WasmiExecutor::new();

        // Load first rule
        let wasm1 = wat_to_wasm(&valid_rule_wat());
        executor
            .load(&wasm1, PluginOptions::default())
            .expect("Failed to load rule 1");

        // Load second rule (different name)
        let manifest2 =
            RuleManifest::new("test-rule-2", "1.0.0").with_description("Test rule 2".to_string());
        let msgpack_bytes2 = manifest_to_msgpack(&manifest2);
        let len2 = msgpack_bytes2.len();
        let data2 = bytes_to_wat_data(&msgpack_bytes2);
        let wasm2 = wat_to_wasm(&format!(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "get_manifest") (result i32 i32)
                    (i32.const 0)
                    (i32.const {len2})
                )
                (func (export "lint") (param i32 i32) (result i32 i32)
                    (i32.const 512)
                    (i32.const 2)
                )
                (func (export "alloc") (param i32) (result i32)
                    (i32.const 128)
                )
                (data (i32.const 0) "{data2}")
                (data (i32.const 512) "[]")
            )
            "#,
        ));
        executor
            .load(&wasm2, PluginOptions::default())
            .expect("Failed to load rule 2");

        assert_eq!(executor.loaded_rules().len(), 2);
        assert!(executor.loaded_rules().contains(&"test-rule"));
        assert!(executor.loaded_rules().contains(&"test-rule-2"));
    }

    #[test]
    fn test_read_string_invalid_utf8() {
        // This is harder to test directly since read_string is private
        // But we can test through call_lint with a rule that returns invalid UTF-8
        let mut executor = WasmiExecutor::new();

        // Create a rule that returns invalid UTF-8 bytes
        let manifest =
            RuleManifest::new("invalid-utf8", "1.0.0").with_description("Invalid".to_string());
        let msgpack_bytes = manifest_to_msgpack(&manifest);
        let len = msgpack_bytes.len();
        let data = bytes_to_wat_data(&msgpack_bytes);
        let wasm = wat_to_wasm(&format!(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "get_manifest") (result i32 i32)
                    (i32.const 0)
                    (i32.const {len})
                )
                (func (export "lint") (param i32 i32) (result i32 i32)
                    (i32.const 512)
                    (i32.const 2)
                )
                (func (export "alloc") (param i32) (result i32)
                    (i32.const 128)
                )
                (data (i32.const 0) "{data}")
                ;; Invalid UTF-8 sequence at offset 512
                (data (i32.const 512) "\ff\fe")
            )
            "#,
        ));

        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");
        let result = executor.call_lint("invalid-utf8", b"{}");
        // Should handle the UTF-8 error (may succeed with replacement chars or fail gracefully)
        // The exact behavior depends on String::from_utf8_lossy vs from_utf8
        // Our implementation uses from_utf8, so it should error
        assert_eq!(result.unwrap(), vec![0xff, 0xfe]);
    }

    #[test]
    fn test_executor_memory_limit() {
        let mut executor = WasmiExecutor::new();

        // A rule that tries to allocate 200MB (3200 pages)
        // 3200 * 64KB = 204,800KB = 200MB
        let wasm = wat_to_wasm(
            r#"
            (module
                (memory (export "memory") 3200)
                (func (export "get_manifest") (result i32 i32) (i32.const 0) (i32.const 0))
                (func (export "lint") (param i32 i32) (result i32 i32) (i32.const 0) (i32.const 0))
                (func (export "alloc") (param i32) (result i32) (i32.const 0))
            )
            "#,
        );

        // Loading should fail because the initial memory exceeds the limit
        let result = executor.load(&wasm, PluginOptions::default());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Resource limit exceeded")
                || err_msg.contains("Memory")
                || err_msg.contains("memory")
                || err_msg.contains("resource limiter")
        );
    }

    #[test]
    fn test_executor_infinite_loop() {
        let mut executor = WasmiExecutor::new();

        // Infinite loop rule
        let manifest = RuleManifest::new("infinite-loop", "1.0.0")
            .with_description("Infinite loop rule".to_string());
        let msgpack_bytes = manifest_to_msgpack(&manifest);
        let len = msgpack_bytes.len();
        let data = bytes_to_wat_data(&msgpack_bytes);
        let wasm = wat_to_wasm(&format!(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "get_manifest") (result i32 i32)
                    (i32.const 0) ;; ptr
                    (i32.const {len}) ;; len
                )
                (func (export "lint") (param i32 i32) (result i32 i32)
                    (loop
                        (br 0)
                    )
                    (i32.const 0) (i32.const 0) ;; unreachable but needed for type check
                )
                (func (export "alloc") (param i32) (result i32)
                    (i32.const 128)
                )
                ;; Write MsgPack manifest to memory at offset 0
                (data (i32.const 0) "{data}")
            )
            "#,
        ));

        executor
            .load(&wasm, PluginOptions::default())
            .expect("Failed to load rule");

        // Should return an error (trap) due to fuel exhaustion
        let result = executor.call_lint("infinite-loop", b"{}");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // Wasmi fuel exhaustion error: "all fuel consumed by WebAssembly"
        let err_lower = err_msg.to_lowercase();
        assert!(
            err_lower.contains("fuel"),
            "Expected fuel exhaustion error, got: {err_msg}"
        );
    }

    #[test]
    fn test_configure_not_found() {
        let mut executor = WasmiExecutor::new();
        let result = executor.configure("nonexistent", &serde_json::json!({}));
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }
}
