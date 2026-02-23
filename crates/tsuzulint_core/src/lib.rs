//! # tsuzulint_core
//!
//! Core linter engine for TsuzuLint.
//!
//! This crate provides:
//! - The main `Linter` orchestrator
//! - Configuration loading
//! - File discovery and filtering
//! - Parallel processing
//!
//! ## Example
//!
//! ```rust,ignore
//! use tsuzulint_core::{Linter, LinterConfig};
//!
//! let config = LinterConfig::from_file(".tsuzulint.json")?;
//! let linter = Linter::new(config)?;
//!
//! let results = linter.lint_files(&["src/**/*.md"])?;
//! for result in results {
//!     println!("{}: {} issues", result.path.display(), result.diagnostics.len());
//! }
//! ```

mod block_extractor;
mod config;
pub mod context;
mod diagnostic_dist;
mod error;
pub mod file_finder;
mod file_linter;
mod fix;
mod fixer;
pub mod formatters;
mod ignore_range;
mod linter;
mod manifest_resolver;
mod parallel_linter;
pub mod pool;
pub mod resolver;
mod result;
mod rule_loader;
pub mod rule_manifest;
pub mod walker;

pub use config::{
    CacheConfig, CacheConfigDetail, LinterConfig, RuleDefinition, RuleDefinitionDetail,
};
pub use context::{CodeBlockInfo, DocumentStructure, HeadingInfo, LineInfo, LinkInfo, LintContext};
pub use error::LinterError;
pub use fix::{DependencyGraph, FixCoordinator, FixResult};
pub use fixer::{FixerResult, apply_fixes_to_content, apply_fixes_to_file};
pub use formatters::generate_sarif;
pub use linter::{LintFilesResult, Linter};
pub use pool::{PluginHostPool, PooledHost};
pub use result::LintResult;

#[cfg(test)]
pub mod test_utils;

pub use tsuzulint_plugin::{Diagnostic, Fix, Severity};
