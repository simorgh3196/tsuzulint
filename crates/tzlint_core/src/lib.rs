//! `tzlint_core` — parser, lint engine, config, cache, and centralized I/O.
//!
//! Compiles for native and `wasm32`. Houses:
//! - the markdown-rs parser + mdast → index-AST transform,
//! - the single-traversal multi-visitor `Engine::lint` (the one dispatch entry point),
//! - multi-format config loading (+ presets), the document-level cache,
//! - the position mapper, and the centralized boundary `io` module (`read_with_limit`,
//!   atomic writes), behind a `Host` provider abstraction so embedders inject their
//!   environment (native fs / Node / browser).
//!
//! Landed so far: the [`parse`] function + [`LineIndex`] position mapper (M1b), and the
//! single-traversal [`Engine`] + autofix [`fix`]/[`apply_fixes`] (M1c-2). TODO(M1): config,
//! cache, and io.

pub mod engine;
pub mod fix;
pub mod parse;
pub mod position;

pub use engine::Engine;
pub use fix::{FixPass, MAX_FIX_PASSES, apply_fixes, fix};
pub use parse::{ParseError, parse};
pub use position::LineIndex;
