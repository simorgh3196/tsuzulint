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
//! Landed so far (M1b): the [`parse`] function (markdown-rs + mdast → index-AST transform)
//! and the [`LineIndex`] position mapper. TODO(M1): the engine, config, cache, and io.

pub mod parse;
pub mod position;

pub use parse::{ParseError, parse};
pub use position::LineIndex;
