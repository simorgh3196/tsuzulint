//! Rule executor abstraction.
//!
//! This module provides the `RuleExecutor` trait which abstracts
//! the WASM runtime implementation, allowing different backends
//! for native (Extism) and browser (wasmi) environments.

use crate::{PluginError, RuleManifest};

/// Result from loading a WASM rule.
#[derive(Debug)]
pub struct LoadResult {
    /// The rule name extracted from the manifest.
    pub name: String,
    /// The rule manifest.
    pub manifest: RuleManifest,
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
    ///
    /// # Returns
    ///
    /// The rule name and manifest on success.
    fn load(&mut self, wasm_bytes: &[u8]) -> Result<LoadResult, PluginError>;

    /// Loads a WASM rule from a file path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the WASM file
    ///
    /// # Returns
    ///
    /// The rule name and manifest on success.
    fn load_file(&mut self, path: &std::path::Path) -> Result<LoadResult, PluginError> {
        let wasm_bytes = std::fs::read(path)?;
        self.load(&wasm_bytes)
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
