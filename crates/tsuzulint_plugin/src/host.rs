//! Plugin host for running WASM rules.
//!
//! This module provides the `PluginHost` which loads and executes
//! WASM-based lint rules using the appropriate executor based on
//! the target environment.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;

// RuleExecutor trait is used by the Executor type alias
#[allow(unused_imports)]
use crate::executor::RuleExecutor;
use crate::{Diagnostic, PluginError, RuleManifest};

#[cfg(feature = "native")]
use crate::executor_extism::ExtismExecutor;

#[cfg(all(feature = "browser", not(feature = "native")))]
use crate::executor_wasmi::WasmiExecutor;

// Type alias for the executor based on feature flags
// Note: When both features are enabled (e.g., in workspace builds),
// native takes precedence. For browser-only builds, ensure only
// the 'browser' feature is enabled.
#[cfg(feature = "native")]
type Executor = ExtismExecutor;

#[cfg(all(feature = "browser", not(feature = "native")))]
type Executor = WasmiExecutor;

// Compile-time error if neither feature is enabled
#[cfg(not(any(feature = "native", feature = "browser")))]
compile_error!("Either 'native' or 'browser' feature must be enabled.");

/// Request sent to a rule's lint function.
#[derive(Debug, Serialize)]
struct LintRequest<'a, T: Serialize> {
    /// The node to lint (serialized).
    node: &'a T,
    /// Rule configuration.
    config: serde_json::Value,
    /// Source text.
    #[serde(borrow)]
    source: &'a serde_json::value::RawValue,
    /// File path (if available).
    file_path: Option<&'a str>,
}

/// Response from a rule's lint function.
#[derive(Debug, Deserialize)]
struct LintResponse {
    /// Diagnostics reported by the rule.
    diagnostics: Vec<Diagnostic>,
}

/// Host for loading and executing WASM rule plugins.
///
/// # Example
///
/// ```rust,ignore
/// use tsuzulint_plugin::PluginHost;
///
/// let mut host = PluginHost::new();
///
/// // Load a rule from a WASM file
/// host.load_rule("./rules/no-todo.wasm")?;
///
/// // Run the rule on an AST node
/// let diagnostics = host.run_rule("no-todo", &node_json, source)?;
/// ```
pub struct PluginHost {
    /// The WASM executor.
    executor: Executor,
    /// Rule manifests by name.
    manifests: HashMap<String, RuleManifest>,
    /// Rule configurations by name.
    configs: HashMap<String, serde_json::Value>,
    /// Aliases mapping (alias -> real_name).
    aliases: HashMap<String, String>,
}

impl PluginHost {
    /// Creates a new plugin host.
    pub fn new() -> Self {
        Self {
            executor: Executor::new(),
            manifests: HashMap::new(),
            configs: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Loads a rule from a WASM file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the WASM file
    ///
    /// # Returns
    ///
    /// The rule manifest on success.
    pub fn load_rule(&mut self, path: impl AsRef<Path>) -> Result<RuleManifest, PluginError> {
        let result = self.executor.load_file(path.as_ref())?;

        self.manifests
            .insert(result.name.clone(), result.manifest.clone());
        self.configs
            .insert(result.name.clone(), serde_json::Value::Null);

        Ok(result.manifest)
    }

    /// Loads a rule from WASM bytes.
    ///
    /// # Arguments
    ///
    /// * `wasm_bytes` - The WASM binary content
    ///
    /// # Returns
    ///
    /// The rule manifest on success.
    pub fn load_rule_bytes(&mut self, wasm_bytes: &[u8]) -> Result<RuleManifest, PluginError> {
        let result = self.executor.load(wasm_bytes)?;

        self.manifests
            .insert(result.name.clone(), result.manifest.clone());
        self.configs
            .insert(result.name.clone(), serde_json::Value::Null);

        Ok(result.manifest)
    }

    /// Renames a loaded rule, optionally updating its manifest.
    ///
    /// # Arguments
    ///
    /// * `old_name` - Current name of the rule
    /// * `new_name` - New name for the rule
    /// * `manifest` - Optional new manifest to associate (overrides existing)
    pub fn rename_rule(
        &mut self,
        old_name: &str,
        new_name: &str,
        manifest: Option<RuleManifest>,
    ) -> Result<(), PluginError> {
        // Resolve old_name to real_name if it's already an alias
        let real_name = self
            .aliases
            .get(old_name)
            .cloned()
            .unwrap_or_else(|| old_name.to_string());

        if !self.manifests.contains_key(old_name) && old_name == real_name {
            // Check if real_name is loaded?
            // Logic: rule is loaded if it is in manifests (under whatever name)
            // If old_name is not in manifests, it's not loaded as such.
            return Err(PluginError::not_found(old_name));
        }

        // Move manifest
        if let Some(old_manifest) = self.manifests.remove(old_name) {
            let new_manifest = manifest.unwrap_or(old_manifest);
            self.manifests.insert(new_name.to_string(), new_manifest);
        } else {
            // If old_name was just an alias to real_name, but not in manifests?
            // Should not happen if we maintain consistency.
            // If we rely on manifests keys as source of truth for "loaded rules exposed to user"
            // Then we must ensure entry exists.
            if let Some(mani) = manifest {
                self.manifests.insert(new_name.to_string(), mani);
            } else {
                return Err(PluginError::not_found(old_name));
            }
        }

        // Update alias map only after successful manifest handling
        self.aliases.insert(new_name.to_string(), real_name);

        // Move config
        if let Some(config) = self.configs.remove(old_name) {
            self.configs.insert(new_name.to_string(), config);
        }

        // Remove old alias if it existed
        if self.aliases.contains_key(old_name) {
            self.aliases.remove(old_name);
        }

        Ok(())
    }

    /// Configures a loaded rule.
    ///
    /// # Arguments
    ///
    /// * `name` - Rule name
    /// * `config` - Configuration value (will be passed to the rule)
    pub fn configure_rule(
        &mut self,
        name: &str,
        config: serde_json::Value,
    ) -> Result<(), PluginError> {
        if !self.manifests.contains_key(name) {
            return Err(PluginError::not_found(name));
        }

        self.configs.insert(name.to_string(), config);
        Ok(())
    }

    /// Gets the manifest for a loaded rule.
    pub fn get_manifest(&self, name: &str) -> Option<&RuleManifest> {
        self.manifests.get(name)
    }

    /// Returns the names of all loaded rules.
    pub fn loaded_rules(&self) -> impl Iterator<Item = &String> {
        self.manifests.keys()
    }

    /// Runs a rule on a node.
    ///
    /// # Arguments
    ///
    /// * `name` - Rule name
    /// * `node` - The AST node (serialized as JSON or a Serializable struct)
    /// * `source` - The source text (serialized as JSON string)
    /// * `file_path` - Optional file path
    ///
    /// # Returns
    ///
    /// Diagnostics reported by the rule.
    pub fn run_rule<T: Serialize>(
        &mut self,
        name: &str,
        node: &T,
        source: &serde_json::value::RawValue,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        Self::run_rule_with_parts(
            &mut self.executor,
            &self.configs,
            &self.aliases,
            name,
            node,
            source,
            file_path,
        )
    }

    /// Internal helper to run a rule with split borrows.
    #[allow(clippy::too_many_arguments)]
    fn run_rule_with_parts<T: Serialize>(
        executor: &mut Executor,
        configs: &HashMap<String, serde_json::Value>,
        aliases: &HashMap<String, String>,
        name: &str,
        node: &T,
        source: &serde_json::value::RawValue,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        let config = configs
            .get(name)
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let request = LintRequest {
            node,
            config,
            source,
            file_path,
        };

        let real_name = aliases.get(name).map(|s| s.as_str()).unwrap_or(name);

        let request_json = serde_json::to_string(&request)?;
        let response_json = executor.call_lint(real_name, &request_json)?;

        let response: LintResponse = serde_json::from_str(&response_json)
            .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;

        Ok(response.diagnostics)
    }

    /// Runs all loaded rules on a node.
    ///
    /// # Arguments
    ///
    /// * `node` - The AST node (serialized as JSON or a Serializable struct)
    /// * `source` - The source text (serialized as JSON string)
    /// * `file_path` - Optional file path
    ///
    /// # Returns
    ///
    /// All diagnostics from all rules.
    pub fn run_all_rules<T: Serialize>(
        &mut self,
        node: &T,
        source: &serde_json::value::RawValue,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        let mut all_diagnostics = Vec::new();

        // Iterate over manifest keys directly without collecting into a Vec.
        // We can do this because run_rule_with_parts takes split borrows,
        // so `self.manifests` (immutable) is not conflicted with `self.executor` (mutable).
        for name in self.manifests.keys() {
            match Self::run_rule_with_parts(
                &mut self.executor,
                &self.configs,
                &self.aliases,
                name,
                node,
                source,
                file_path,
            ) {
                Ok(diagnostics) => {
                    all_diagnostics.extend(diagnostics);
                }
                Err(e) => {
                    warn!("Rule '{}' failed: {}", name, e);
                }
            }
        }

        Ok(all_diagnostics)
    }

    /// Unloads a rule.
    pub fn unload_rule(&mut self, name: &str) -> bool {
        let real_name = self
            .aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string());

        self.manifests.remove(name);
        self.configs.remove(name);
        self.aliases.remove(name);

        // Since rename_rule uses move semantics, unloading is safe.
        self.executor.unload(&real_name)
    }

    /// Unloads all rules.
    pub fn unload_all(&mut self) {
        self.manifests.clear();
        self.configs.clear();
        self.aliases.clear();
        self.executor.unload_all();
    }
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_host_new() {
        let host = PluginHost::new();
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    fn test_plugin_host_default() {
        let host = PluginHost::default();
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    fn test_plugin_host_not_found() {
        let mut host = PluginHost::new();
        let node_json = serde_json::to_string(&serde_json::json!({})).unwrap();
        let node = serde_json::value::RawValue::from_string(node_json).unwrap();
        let source_json = serde_json::to_string("").unwrap();
        let source = serde_json::value::RawValue::from_string(source_json).unwrap();

        let result = host.run_rule("nonexistent", &node, &source, None);
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn test_configure_rule_not_found() {
        let mut host = PluginHost::new();
        let result = host.configure_rule("nonexistent", serde_json::json!({}));
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn test_get_manifest_not_loaded() {
        let host = PluginHost::new();
        assert!(host.get_manifest("nonexistent").is_none());
    }

    #[test]
    fn test_unload_rule_not_found() {
        let mut host = PluginHost::new();
        assert!(!host.unload_rule("nonexistent"));
    }

    #[test]
    fn test_unload_all_empty() {
        let mut host = PluginHost::new();
        host.unload_all();
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_load_and_unload_rule() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load rule
        let manifest = host.load_rule_bytes(&wasm).expect("Failed to load rule");
        assert_eq!(manifest.name, "test-rule");

        // Verify it's in loaded_rules
        let loaded: Vec<&String> = host.loaded_rules().collect();
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains(&&"test-rule".to_string()));

        // Get manifest
        let retrieved_manifest = host.get_manifest("test-rule");
        assert!(retrieved_manifest.is_some());
        assert_eq!(retrieved_manifest.unwrap().name, "test-rule");

        // Unload rule
        assert!(host.unload_rule("test-rule"));
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_configure_and_run_rule() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load rule
        host.load_rule_bytes(&wasm).expect("Failed to load rule");

        // Configure rule
        let config = serde_json::json!({"key": "value"});
        host.configure_rule("test-rule", config.clone())
            .expect("Failed to configure rule");

        // Verify configuration was set
        assert_eq!(host.configs.get("test-rule").unwrap(), &config);
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_rename_rule() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load rule
        host.load_rule_bytes(&wasm).expect("Failed to load rule");

        // Rename rule
        host.rename_rule("test-rule", "renamed-rule", None)
            .expect("Failed to rename rule");

        // Verify old name is gone
        assert!(host.get_manifest("test-rule").is_none());

        // Verify new name exists
        let manifest = host.get_manifest("renamed-rule");
        assert!(manifest.is_some());
        assert_eq!(manifest.unwrap().name, "test-rule"); // Internal name unchanged
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_rename_rule_with_new_manifest() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load rule
        host.load_rule_bytes(&wasm).expect("Failed to load rule");

        // Create new manifest
        let new_manifest = RuleManifest {
            name: "custom-name".to_string(),
            version: "2.0.0".to_string(),
            description: Some("Custom description".to_string()),
            fixable: true,
            node_types: vec!["Custom".to_string()],
            isolation_level: crate::IsolationLevel::Global,
            schema: None,
        };

        // Rename with new manifest
        host.rename_rule("test-rule", "custom-rule", Some(new_manifest.clone()))
            .expect("Failed to rename rule");

        // Verify new manifest is used
        let retrieved_manifest = host.get_manifest("custom-rule");
        assert!(retrieved_manifest.is_some());
        assert_eq!(retrieved_manifest.unwrap().name, "custom-name");
        assert_eq!(retrieved_manifest.unwrap().version, "2.0.0");
    }

    #[test]
    fn test_rename_rule_not_found() {
        let mut host = PluginHost::new();
        let result = host.rename_rule("nonexistent", "new-name", None);
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_run_all_rules_empty() {
        let mut host = PluginHost::new();
        let node = serde_json::json!({"type": "Str", "value": "test"});
        let node_json = serde_json::to_string(&node).unwrap();
        let node_raw = serde_json::value::RawValue::from_string(node_json).unwrap();
        let source_json = serde_json::to_string("test").unwrap();
        let source_raw = serde_json::value::RawValue::from_string(source_json).unwrap();

        let result = host.run_all_rules(&node_raw, &source_raw, None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_run_all_rules_with_multiple_rules() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();

        // Load first rule
        let wasm1 = wat_to_wasm(&valid_rule_wat());
        host.load_rule_bytes(&wasm1).expect("Failed to load rule 1");

        // Load second rule with different name
        let json2 = r#"{"name":"test-rule-2","version":"1.0.0","description":"Test rule 2"}"#;
        let len2 = json2.len();
        let wat2 = format!(
            r#"
            (module
                (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
                (memory (export "memory") 1)
                (func (export "get_manifest")
                    (call $output_set (i64.const 0) (i64.const {}))
                )
                (func (export "lint")
                    (call $output_set (i64.const 100) (i64.const 2))
                )
                (func (export "alloc") (param i64) (result i64)
                    (i64.const 128)
                )
                (data (i32.const 0) "{}")
                (data (i32.const 100) "[]")
            )
            "#,
            len2,
            json2.replace("\"", "\\\"")
        );
        let wasm2 = wat_to_wasm(&wat2);
        host.load_rule_bytes(&wasm2).expect("Failed to load rule 2");

        // Run all rules
        let node = serde_json::json!({"type": "Str", "value": "test"});
        let node_json = serde_json::to_string(&node).unwrap();
        let node_raw = serde_json::value::RawValue::from_string(node_json).unwrap();
        let source_json = serde_json::to_string("test").unwrap();
        let source_raw = serde_json::value::RawValue::from_string(source_json).unwrap();

        let result = host.run_all_rules(&node_raw, &source_raw, None);
        assert!(result.is_ok());

        // Verify both rules were loaded
        let loaded: Vec<&String> = host.loaded_rules().collect();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_load_rule_from_file() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};
        use std::io::Write;

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Write WASM to temporary file
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(&wasm).unwrap();
        let path = temp_file.path();

        // Load from file
        let manifest = host.load_rule(path).expect("Failed to load rule from file");
        assert_eq!(manifest.name, "test-rule");
    }

    #[test]
    fn test_load_rule_from_nonexistent_file() {
        let mut host = PluginHost::new();
        let result = host.load_rule("/nonexistent/path/to/rule.wasm");
        assert!(result.is_err());
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_unload_all_with_multiple_rules() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();

        // Load multiple rules
        let wasm = wat_to_wasm(&valid_rule_wat());
        host.load_rule_bytes(&wasm).expect("Failed to load rule");

        // Verify loaded
        assert_eq!(host.loaded_rules().count(), 1);

        // Unload all
        host.unload_all();

        // Verify all unloaded
        assert_eq!(host.loaded_rules().count(), 0);
        assert!(host.get_manifest("test-rule").is_none());
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_rename_preserves_config() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load and configure rule
        host.load_rule_bytes(&wasm).expect("Failed to load rule");
        let config = serde_json::json!({"test": "value"});
        host.configure_rule("test-rule", config.clone())
            .expect("Failed to configure");

        // Rename
        host.rename_rule("test-rule", "new-rule", None)
            .expect("Failed to rename");

        // Verify config was moved
        assert_eq!(host.configs.get("new-rule").unwrap(), &config);
        assert!(!host.configs.contains_key("test-rule"));
    }

    #[test]
    #[cfg(all(feature = "test-utils", feature = "native"))]
    fn test_double_unload() {
        use crate::test_utils::{valid_rule_wat, wat_to_wasm};

        let mut host = PluginHost::new();
        let wasm = wat_to_wasm(&valid_rule_wat());

        // Load rule
        host.load_rule_bytes(&wasm).expect("Failed to load rule");

        // First unload should succeed
        assert!(host.unload_rule("test-rule"));

        // Second unload should return false
        assert!(!host.unload_rule("test-rule"));
    }
}