//! Extism-based WASM executor for native environments.
//!
//! This module provides high-performance WASM execution using Extism,
//! which internally uses wasmtime for JIT compilation.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use extism::{Manifest, Plugin, PluginBuilder, Wasm};
use extism_manifest::MemoryOptions;
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::executor::{LoadResult, PluginOptions, RuleExecutor};
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

    /// Configures the manifest with security limits and options.
    fn configure_manifest(&self, mut manifest: Manifest, options: &PluginOptions) -> Manifest {
        // Set execution timeout
        manifest.timeout_ms = options.timeout_ms.or(Some(self.timeout_ms));

        // Set memory limits
        manifest.memory = MemoryOptions {
            max_pages: options.memory_max_pages.or(Some(DEFAULT_MEMORY_MAX_PAGES)),
            max_http_response_bytes: options.memory_max_http_response_bytes.or(Some(0)),
            max_var_bytes: Some(1024 * 1024), // Limit variable storage to 1MB
        };

        // Set allowed network hosts
        manifest.allowed_hosts = options.allowed_hosts.clone().or(Some(vec![]));

        // Set allowed file system paths
        manifest.allowed_paths = options.allowed_paths.clone().or(Some(BTreeMap::new()));

        // Set configuration variables
        manifest.config = options.config.clone();

        manifest
    }

    fn build_plugin(
        &self,
        source: &RuleSource,
        options: &PluginOptions,
    ) -> Result<Plugin, PluginError> {
        let wasm = source.to_wasm();
        let manifest = Manifest::new([wasm]);
        let manifest = self.configure_manifest(manifest, options);

        // Fetch Wasmtime JIT cache configuration if available
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

    fn load_rule(
        &mut self,
        source: RuleSource,
        options: PluginOptions,
    ) -> Result<LoadResult, PluginError> {
        let mut plugin = self.build_plugin(&source, &options)?;
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
    fn load(
        &mut self,
        wasm_bytes: &[u8],
        options: PluginOptions,
    ) -> Result<LoadResult, PluginError> {
        info!("Loading WASM rule ({} bytes)", wasm_bytes.len());

        let mut hasher = Sha256::new();
        hasher.update(wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        self.load_rule(
            RuleSource::Bytes {
                wasm: Arc::from(wasm_bytes),
                hash,
            },
            options,
        )
    }

    fn load_file(
        &mut self,
        path: &Path,
        options: PluginOptions,
    ) -> Result<LoadResult, PluginError> {
        info!("Loading rule from file: {}", path.display());

        // Read the file to calculate the hash, but don't keep the bytes in memory
        // if we are using RuleSource::File.
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| PluginError::load(format!("Failed to read file: {}", e)))?;

        let mut hasher = Sha256::new();
        hasher.update(&wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        self.load_rule(
            RuleSource::File {
                path: path.to_path_buf(),
                hash,
            },
            options,
        )
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
