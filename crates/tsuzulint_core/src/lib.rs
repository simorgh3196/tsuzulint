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

mod config;
mod error;
mod fixer;
pub mod formatters;
mod linter;
pub mod resolver;
mod result;

pub use config::{LinterConfig, RuleDefinition, RuleDefinitionDetail};
pub use error::LinterError;
pub use fixer::{FixerResult, apply_fixes_to_content, apply_fixes_to_file};
pub use formatters::generate_sarif;
pub use linter::Linter;
pub use result::LintResult;

// Re-export commonly used types
pub use tsuzulint_plugin::{Diagnostic, Fix, Severity};
pub mod rule_manifest;
