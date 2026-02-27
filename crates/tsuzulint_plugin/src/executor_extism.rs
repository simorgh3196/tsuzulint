//! Extism-based WASM executor for native environments.
//!
//! This module provides high-performance WASM execution using Extism,
//! which internally uses wasmtime for JIT compilation.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use extism::{Manifest, Plugin, PluginBuilder, Wasm};
use sha2::{Digest, Sha256};
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
    Bytes { wasm: Arc<[u8]>, hash: String },
    File { path: PathBuf, hash: String },
}

impl RuleSource {
    fn to_wasm(&self) -> Wasm {
        match self {
            RuleSource::Bytes { wasm, hash } => Wasm::data(wasm.to_vec()).with_hash(hash),
            RuleSource::File { path, hash } => Wasm::file(path).with_hash(hash),
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

    fn build_plugin(&self, source: &RuleSource) -> Result<Plugin, PluginError> {
        let wasm = source.to_wasm();
        let manifest = Manifest::new([wasm]);
        let manifest = self.configure_manifest(manifest);

        let mut builder = PluginBuilder::new(manifest).with_wasi(true);

        if let Some(limit) = self.fuel_limit {
            builder = builder.with_fuel_limit(limit);
        }

        builder
            .build()
            .map_err(|e| PluginError::load(format!("Failed to create plugin: {}", e)))
    }

    fn fetch_manifest(plugin: &mut Plugin) -> Result<RuleManifest, PluginError> {
        // Get the rule manifest by calling get_manifest()
        let manifest_bytes: Vec<u8> = plugin
            .call("get_manifest", "")
            .map_err(|e| PluginError::call(format!("Failed to get manifest: {}", e)))?;

        rmp_serde::from_slice(&manifest_bytes)
            .map_err(|e| PluginError::invalid_manifest(e.to_string()))
    }

    fn load_rule(&mut self, source: RuleSource) -> Result<LoadResult, PluginError> {
        let mut plugin = self.build_plugin(&source)?;
        let rule_manifest = Self::fetch_manifest(&mut plugin)?;

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

        let mut hasher = Sha256::new();
        hasher.update(wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        self.load_rule(RuleSource::Bytes {
            wasm: Arc::from(wasm_bytes),
            hash,
        })
    }

    fn load_file(&mut self, path: &Path) -> Result<LoadResult, PluginError> {
        info!("Loading rule from file: {}", path.display());

        // Read the file to calculate the hash, but don't keep the bytes in memory
        // if we are using RuleSource::File.
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| PluginError::load(format!("Failed to read file: {}", e)))?;

        let mut hasher = Sha256::new();
        hasher.update(&wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        self.load_rule(RuleSource::File {
            path: path.to_path_buf(),
            hash,
        })
    }

    fn configure(
        &mut self,
        _rule_name: &str,
        _config: &serde_json::Value,
    ) -> Result<(), PluginError> {
        // Config is now passed via LintRequest, no plugin rebuild needed
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
