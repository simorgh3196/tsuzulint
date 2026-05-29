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
//! TODO(M1): implement the engine, parser transform, config, cache, position mapper.

pub mod io;
