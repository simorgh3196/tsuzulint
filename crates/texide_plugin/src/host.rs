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

#[cfg(feature = "browser")]
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
struct LintRequest<'a> {
    /// The node to lint (serialized).
    node: &'a serde_json::Value,
    /// Rule configuration.
    config: serde_json::Value,
    /// Source text.
    source: &'a str,
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
/// use texide_plugin::PluginHost;
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
}

impl PluginHost {
    /// Creates a new plugin host.
    pub fn new() -> Self {
        Self {
            executor: Executor::new(),
            manifests: HashMap::new(),
            configs: HashMap::new(),
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
    pub fn loaded_rules(&self) -> Vec<&str> {
        self.executor.loaded_rules()
    }

    /// Runs a rule on a node.
    ///
    /// # Arguments
    ///
    /// * `name` - Rule name
    /// * `node` - The AST node (serialized as JSON)
    /// * `source` - The source text
    /// * `file_path` - Optional file path
    ///
    /// # Returns
    ///
    /// Diagnostics reported by the rule.
    pub fn run_rule(
        &mut self,
        name: &str,
        node: &serde_json::Value,
        source: &str,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        let config = self
            .configs
            .get(name)
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let request = LintRequest {
            node,
            config,
            source,
            file_path,
        };

        let request_json = serde_json::to_string(&request)?;
        let response_json = self.executor.call_lint(name, &request_json)?;

        let response: LintResponse = serde_json::from_str(&response_json)
            .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;

        Ok(response.diagnostics)
    }

    /// Runs all loaded rules on a node.
    ///
    /// # Arguments
    ///
    /// * `node` - The AST node (serialized as JSON)
    /// * `source` - The source text
    /// * `file_path` - Optional file path
    ///
    /// # Returns
    ///
    /// All diagnostics from all rules.
    pub fn run_all_rules(
        &mut self,
        node: &serde_json::Value,
        source: &str,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        let rule_names: Vec<String> = self
            .executor
            .loaded_rules()
            .into_iter()
            .map(|s: &str| s.to_string())
            .collect();
        let mut all_diagnostics = Vec::new();

        for name in &rule_names {
            match self.run_rule(name, node, source, file_path) {
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
        self.manifests.remove(name);
        self.configs.remove(name);
        self.executor.unload(name)
    }

    /// Unloads all rules.
    pub fn unload_all(&mut self) {
        self.manifests.clear();
        self.configs.clear();
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
        assert!(host.loaded_rules().is_empty());
    }

    #[test]
    fn test_plugin_host_not_found() {
        let mut host = PluginHost::new();
        let node = serde_json::json!({});
        let result = host.run_rule("nonexistent", &node, "", None);
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn test_configure_rule_not_found() {
        let mut host = PluginHost::new();
        let result = host.configure_rule("nonexistent", serde_json::json!({}));
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }
}
