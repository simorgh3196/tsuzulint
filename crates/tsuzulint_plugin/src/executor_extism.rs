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
use tracing::{debug, info, warn};

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
}

impl RuleSource {
    fn to_wasm(&self) -> Wasm {
        match self {
            RuleSource::Bytes { wasm, hash } => Wasm::data(wasm.to_vec()).with_hash(hash),
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

/// Prepares the Wasmtime JIT cache directory layout under `cache_root` and
/// returns the path to the cache `config.toml` on success.
///
/// Layout:
/// ```text
/// <cache_root>/
///   config.toml
///   data/            <-- wasmtime stores compiled artifacts here
/// ```
///
/// Returns `None` if the directory or config file could not be created.
/// Failures are logged via `tracing::warn!` rather than silently swallowed so
/// operators can diagnose cache-related performance issues.
fn prepare_cache_config(cache_root: &Path) -> Option<PathBuf> {
    if let Err(e) = std::fs::create_dir_all(cache_root) {
        warn!(
            error = %e,
            path = %cache_root.display(),
            "wasmtime_cache_root_create_failed"
        );
        return None;
    }

    let data_dir = cache_root.join("data");
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        warn!(
            error = %e,
            path = %data_dir.display(),
            "wasmtime_cache_data_dir_create_failed"
        );
        return None;
    }

    let cache_config = cache_root.join("config.toml");
    if !cache_config.exists() {
        let contents = format!(
            "[cache]\ndirectory = \"{}\"\n",
            data_dir.display().to_string().replace('\\', "\\\\")
        );
        if let Err(e) = std::fs::write(&cache_config, contents) {
            warn!(
                error = %e,
                path = %cache_config.display(),
                "wasmtime_cache_config_write_failed"
            );
            return None;
        }
    }

    Some(cache_config)
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
    fn configure_manifest(&self, mut manifest: Manifest, options: PluginOptions) -> Manifest {
        // Set execution timeout
        manifest.timeout_ms = options.timeout_ms.or(Some(self.timeout_ms));

        // Set memory limits
        manifest.memory = MemoryOptions {
            max_pages: options.memory_max_pages.or(Some(DEFAULT_MEMORY_MAX_PAGES)),
            max_http_response_bytes: options.memory_max_http_response_bytes.or(Some(0)),
            max_var_bytes: Some(1024 * 1024), // Limit variable storage to 1MB
        };

        // Set allowed network hosts
        manifest.allowed_hosts = options.allowed_hosts.or(Some(vec![]));

        // Set allowed file system paths
        manifest.allowed_paths = options.allowed_paths.or(Some(BTreeMap::new()));

        // Set configuration variables
        manifest.config = options.config;

        manifest
    }

    fn build_plugin(
        &self,
        source: &RuleSource,
        options: PluginOptions,
    ) -> Result<Plugin, PluginError> {
        let wasm = source.to_wasm();
        let manifest = Manifest::new([wasm]);
        let manifest = self.configure_manifest(manifest, options);

        // Fetch Wasmtime JIT cache configuration if available
        let mut builder = PluginBuilder::new(manifest).with_wasi(true);

        // Enable Wasmtime JIT compilation caching
        // This dramatically reduces startup time for repeated instantiations of the same WASM.
        let cache_root = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("tsuzulint")
            .join("wasmtime_cache");

        if let Some(cache_config) = prepare_cache_config(&cache_root) {
            builder = builder.with_cache_config(&cache_config);
        }

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
        let mut plugin = self.build_plugin(&source, options)?;
        let rule_manifest = Self::fetch_manifest(&mut plugin)?;

        debug!(
            "Loaded rule: {} v{}",
            rule_manifest.name, rule_manifest.version
        );

        self.rules.insert(
            rule_manifest.name.clone(),
            LoadedRule {
                plugin,
                manifest: rule_manifest.clone(),
            },
        );

        Ok(LoadResult {
            name: rule_manifest.name.clone(),
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
        let wasm_bytes = crate::executor::read_wasm_file_or_error(path)?;

        let mut hasher = Sha256::new();
        hasher.update(&wasm_bytes);
        let hash = hex::encode(hasher.finalize());

        self.load_rule(
            RuleSource::Bytes {
                wasm: Arc::from(wasm_bytes),
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn prepare_cache_config_creates_layout_on_miss() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("wasmtime_cache");
        assert!(!root.exists(), "precondition: cache root should not exist");

        let cfg = prepare_cache_config(&root).expect("cache config should be created on miss");

        assert_eq!(cfg, root.join("config.toml"));
        assert!(cfg.is_file(), "config.toml should exist after miss");
        assert!(root.join("data").is_dir(), "data/ directory should exist");

        let contents = std::fs::read_to_string(&cfg).expect("read config.toml");
        assert!(
            contents.contains("[cache]"),
            "config should contain [cache] section, got: {contents}"
        );
        assert!(
            contents.contains("directory = "),
            "config should reference data directory, got: {contents}"
        );
    }

    #[test]
    fn prepare_cache_config_is_idempotent_on_hit() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("wasmtime_cache");

        // First call: cache miss -- creates the files.
        let cfg1 = prepare_cache_config(&root).expect("first call");
        let original = std::fs::read_to_string(&cfg1).expect("read after first call");
        let mtime1 = std::fs::metadata(&cfg1)
            .and_then(|m| m.modified())
            .expect("mtime after first call");

        // Second call: cache hit -- must not rewrite the existing config.
        let cfg2 = prepare_cache_config(&root).expect("second call");
        assert_eq!(cfg1, cfg2, "same config path returned on hit");

        let after = std::fs::read_to_string(&cfg2).expect("read after second call");
        assert_eq!(
            original, after,
            "config.toml contents must not change on cache hit"
        );

        let mtime2 = std::fs::metadata(&cfg2)
            .and_then(|m| m.modified())
            .expect("mtime after second call");
        assert_eq!(mtime1, mtime2, "config.toml must not be rewritten on hit");

        // Data dir still present.
        assert!(root.join("data").is_dir());
    }

    #[test]
    fn prepare_cache_config_returns_none_when_root_unwritable() {
        // Point the cache root at a path rooted in a regular file; create_dir_all
        // cannot create a directory underneath a file, so this forces an error.
        let tmp = TempDir::new().expect("tempdir");
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").expect("write blocker file");

        let unreachable_root = blocker.join("nested").join("cache");
        assert!(
            prepare_cache_config(&unreachable_root).is_none(),
            "should return None when cache root cannot be created"
        );
    }
}
