//! `tzlint_pdk` — the rule-author SDK (Plugin Development Kit).
//!
//! Provides the ergonomic, zero-copy [`NodeRef`] cursor over the archived AST and the
//! diagnostic model rules produce ([`Diagnostic`], [`Fix`], [`Severity`], [`RuleId`]) plus
//! [`RuleMeta`]. v1 ships a single Rust PDK; the surface is `no_std` (alloc) so it can sit
//! on the `wasm32` guest side of the plugin ABI.
//!
//! Landed in M1c-1: the diagnostic model + `NodeRef` facade + `RuleMeta`. The `Rule` trait
//! and `Context` land with the engine (M1c-2); the frozen plugin ABI calling convention is
//! M3 (see `abi-spec.md`).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod diagnostic;
mod meta;
mod node;

pub use diagnostic::{Diagnostic, Fix, RuleId, Severity};
pub use meta::RuleMeta;
pub use node::{Children, NodeRef};
