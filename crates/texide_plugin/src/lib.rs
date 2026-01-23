//! # texide_plugin
//!
//! WASM plugin system for Texide.
//!
//! This crate provides:
//! - Plugin loading and management
//! - Host functions for rule execution
//! - Rule manifest handling
//! - Diagnostic collection
//!
//! ## Architecture
//!
//! Rules are compiled to WASM and run in a sandboxed environment.
//! The underlying runtime depends on the target environment:
//!
//! - **Native** (default): Uses Extism/wasmtime for high-performance JIT execution
//! - **Browser**: Uses wasmi for pure Rust interpretation (WASM-in-WASM)
//!
//! ## Features
//!
//! - `native` (default): Enable Extism backend for native environments
//! - `browser`: Enable wasmi backend for browser/WASM environments
//!
//! ## Example
//!
//! ```rust,ignore
//! use texide_plugin::PluginHost;
//!
//! let mut host = PluginHost::new();
//! host.load_rule("./rules/no-todo.wasm")?;
//!
//! let diagnostics = host.run_rule("no-todo", &ast, &source)?;
//! ```

mod diagnostic;
mod error;
mod executor;
mod host;
mod manifest;

#[cfg(feature = "native")]
mod executor_extism;

#[cfg(feature = "browser")]
mod executor_wasmi;

pub use diagnostic::{Diagnostic, Fix, Severity};
pub use error::PluginError;
pub use executor::{LoadResult, RuleExecutor};
pub use host::PluginHost;
pub use manifest::RuleManifest;
