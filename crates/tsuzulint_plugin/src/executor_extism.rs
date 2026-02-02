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
}
