//! # tsuzulint_plugin
//!
//! WASM plugin system for TsuzuLint.
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
//! use tsuzulint_plugin::PluginHost;
//!
//! let mut host = PluginHost::new();
//! host.load_rule("./rules/no-todo.wasm")?;
//!
//! let diagnostics = host.run_rule("no-todo", &ast, "source", &tokens, &sentences, None)?;
//! ```

mod diagnostic;
mod error;
mod executor;
mod host;
mod manifest;

#[cfg(feature = "native")]
mod executor_extism;

#[cfg(all(feature = "browser", not(feature = "native")))]
mod executor_wasmi;

#[cfg(feature = "test-utils")]
pub mod test_utils;

pub use diagnostic::{Diagnostic, Fix, Severity};
pub use error::PluginError;
pub use executor::{LoadResult, RuleExecutor};
pub use host::{PluginHost, PreparedLintRequest};
pub use manifest::{Capability, IsolationLevel, KnownLanguage, RuleManifest};
