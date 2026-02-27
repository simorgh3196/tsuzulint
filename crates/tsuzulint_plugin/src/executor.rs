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
}

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
        let wasm_bytes = std::fs::read(path)?;
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
