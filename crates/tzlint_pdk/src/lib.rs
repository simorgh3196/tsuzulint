//! `tzlint_pdk` — the rule-author SDK (Plugin Development Kit).
//!
//! Provides the ergonomic, zero-copy `NodeRef<'ast>` facade over the archived AST, the
//! `Rule`/`RuleMeta`/`Context`/`Diagnostic`/`Fix` surface, authoring macros, and a test
//! harness. v1 ships a single Rust PDK; the ABI is frozen and documented so future
//! TS/AssemblyScript PDKs can be added without redesign.
//!
//! TODO(M3): define the PDK surface against the frozen `abi-spec.md` calling convention.
