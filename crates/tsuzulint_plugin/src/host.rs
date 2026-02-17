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
use tsuzulint_text::{Token, Sentence};

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
    /// Tokens in the text.
    tokens: Vec<Token>,
    /// Sentences in the text.
    sentences: Vec<Sentence>,
    /// The node to lint (serialized).
    node: &'a T,
    /// Rule configuration.
    config: serde_json::Value,
    /// Source text.
    source: String,
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
/// let diagnostics = host.run_rule("no-todo", &node, &source, Some("example.md"))?;
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
    /// * `node` - The AST node (serialized as Msgpack or a Serializable struct)
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
        tokens: &serde_json::value::RawValue,
        sentences: &serde_json::value::RawValue,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        Self::run_rule_with_parts(
            &mut self.executor,
            &self.configs,
            &self.aliases,
            name,
            node,
            source,
            tokens,
            sentences,
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
        tokens: &serde_json::value::RawValue,
        sentences: &serde_json::value::RawValue,
        file_path: Option<&str>,
    ) -> Result<Vec<Diagnostic>, PluginError> {
        let config = configs
            .get(name)
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let source_str: String = serde_json::from_str(source.get())
            .map_err(|e| PluginError::call(format!("Invalid source JSON: {}", e)))?;

        let tokens_vec: Vec<Token> = serde_json::from_str(tokens.get())
            .map_err(|e| PluginError::call(format!("Invalid tokens JSON: {}", e)))?;

        let sentences_vec: Vec<Sentence> = serde_json::from_str(sentences.get())
            .map_err(|e| PluginError::call(format!("Invalid sentences JSON: {}", e)))?;

        let request = LintRequest {
            node,
            config,
            source: source_str,
            tokens: tokens_vec,
            sentences: sentences_vec,
            file_path,
        };

        let real_name = aliases.get(name).map(|s| s.as_str()).unwrap_or(name);

        let request_bytes = rmp_serde::to_vec_named(&request)
            .map_err(|e| PluginError::call(format!("Failed to serialize request: {}", e)))?;

        let response_bytes = executor.call_lint(real_name, &request_bytes)?;

        let response: LintResponse = rmp_serde::from_slice(&response_bytes)
            .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;

        Ok(response.diagnostics)
    }

    /// Runs all loaded rules on a node.
    ///
    /// # Arguments
    ///
    /// * `node` - The AST node (serialized as Msgpack or a Serializable struct)
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
        tokens: &serde_json::value::RawValue,
        sentences: &serde_json::value::RawValue,
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
                tokens,
                sentences,
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
    fn test_plugin_host_not_found() {
        let mut host = PluginHost::new();
        let node_bytes = serde_json::to_string(&serde_json::json!({})).unwrap();
        let node = serde_json::value::RawValue::from_string(node_bytes).unwrap();
        let source_json = serde_json::to_string("").unwrap();
        let source = serde_json::value::RawValue::from_string(source_json).unwrap();

        let tokens_raw = serde_json::value::RawValue::from_string(serde_json::to_string(&Vec::<Token>::new()).unwrap()).unwrap();
        let sentences_raw = serde_json::value::RawValue::from_string(serde_json::to_string(&Vec::<Sentence>::new()).unwrap()).unwrap();
        let result = host.run_rule("nonexistent", &node, &source, &tokens_raw, &sentences_raw, None);
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn test_configure_rule_not_found() {
        let mut host = PluginHost::new();
        let result = host.configure_rule("nonexistent", serde_json::json!({}));
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn test_serialization_compat_with_pdk() {
        // This test simulates the serialization of LintRequest in the host
        // and deserialization in the PDK (using a mock struct that matches PDK).

        use serde::Deserialize;

        // Mock of the PDK's LintRequest struct
        #[derive(Debug, Clone, Deserialize)]
        struct PdkLintRequest {
            pub node: serde_json::Value,
            pub config: serde_json::Value,
            pub source: String,
            pub tokens: Vec<Token>,
            pub sentences: Vec<Sentence>,
            pub file_path: Option<String>,
            #[serde(default)]
            pub helpers: Option<serde_json::Value>,
        }

        let node_data = serde_json::json!({"type": "Doc", "children": []});
        let config = serde_json::json!({"option": "value"});
        let tokens = vec![];
        let sentences = vec![];
        let source = "test content".to_string();
        let file_path = Some("test.md");

        // Host side
        let host_request = LintRequest {
            node: &node_data,
            config: config.clone(),
            source: source.clone(),
            tokens: tokens.clone(),
            sentences: sentences.clone(),
            file_path,
        };

        // Serialize using rmp_serde (as done in host)
        let bytes = rmp_serde::to_vec_named(&host_request).expect("Serialization failed");

        // Guest side (deserialize using rmp_serde)
        let guest_request: PdkLintRequest =
            rmp_serde::from_slice(&bytes).expect("Deserialization failed");

        // Verify content
        assert_eq!(guest_request.source, source);
        assert_eq!(guest_request.tokens, tokens);
        assert_eq!(guest_request.sentences, sentences);
        assert_eq!(guest_request.file_path, file_path.map(|s| s.to_string()));
        assert_eq!(guest_request.config, config);
        assert_eq!(guest_request.node, node_data);
        assert_eq!(guest_request.helpers, None);
    }
}
