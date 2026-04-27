//! Native (built-in) rule engine.
//!
//! Native rules run as Rust code compiled into the linter binary, without
//! going through the WASM plugin boundary. This is the fastest execution path
//! and is the home of rules that are (a) broadly useful, (b) expressible with
//! the standard library + `tsuzulint_text`, and (c) stable enough that shipping
//! them in the binary is reasonable.
//!
//! WASM plugins remain the right fit for user-specific rules, experimental
//! rules, or rules that depend on user-provided dictionaries.
//!
//! # Architecture
//!
//! - [`Rule`] — the trait every native rule implements.
//! - [`RuleContext`] — carries the parsed AST, source text, morphological
//!   tokens, sentences, and the per-rule options into the rule.
//! - [`BuiltinRegistry`] — a process-global registry of all built-in rules,
//!   looked up by name.
//!
//! The linter pipeline consults [`BuiltinRegistry::get`] for each name listed
//! in `LinterConfig.builtin_rules`, then invokes the rule with a `RuleContext`
//! assembled from the current file's AST.

mod context;
mod registry;
mod rule_trait;
mod rules;

pub use context::RuleContext;
pub use registry::{BuiltinRegistry, builtin_registry};
pub use rule_trait::Rule;
