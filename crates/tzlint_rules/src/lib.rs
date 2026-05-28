//! `tzlint_rules` — built-in native rules.
//!
//! Each rule implements the `Rule` trait (from `tzlint_core`) and registers the node
//! kinds it cares about with the single-traversal scheduler. Cross-node rules use the
//! `Context` accumulator + `finish()` hook.
//!
//! TODO(M1): migrate/refactor the legacy native rules and presets (ja-technical-writing,
//! ja-basic) into this crate against the new `Rule` trait and Diagnostic/Fix model.
