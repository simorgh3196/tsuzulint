//! `tzlint_core` — parser, lint engine, config, cache, and centralized I/O.
//!
//! Compiles for native and `wasm32`. Houses:
//! - the markdown-rs parser + mdast → index-AST transform,
//! - the single-traversal multi-visitor `Engine::lint` (the one dispatch entry point),
//! - multi-format config loading (+ presets), the document-level cache,
//! - the position mapper, and the centralized boundary `io` module (`Host::read_to_string`
//!   with a size cap, `Host::write_atomic`), behind a `Host` provider abstraction so
//!   embedders inject their environment (native fs / Node / browser).
//!
//! Landed so far: the [`parse`] function + [`LineIndex`] position mapper (M1b), the
//! single-traversal [`Engine`] + autofix [`fix`]/[`apply_fixes`] (M1c-2), the centralized
//! [`io`] boundary ([`Host`] + size limits + atomic writes, M1d-1), the multi-format
//! [`config`] loader (discovery + presets + strict validation, M1d-2), the published
//! [`CONFIG_SCHEMA`] (M1d-3), and the in-memory document [`cache`] (M1e; persistence is
//! deferred to M1g behind a `Host`-backed seam).

pub mod cache;
pub mod config;
pub mod dict;
pub mod engine;
pub mod fix;
pub mod io;
pub mod net;
pub mod parse;
pub mod position;

pub use cache::{
    CacheError, CacheKey, CacheKeyInput, DocumentCache, document_cache_key, lint_cached,
};
pub use config::{
    CONFIG_SCHEMA, Config, ConfigError, ConfigFormat, DiscoveredConfig, Preset, RuleSetting,
    ShadowedCandidate, discover, resolve,
};
pub use dict::{DictError, provision_dictionary, provision_dictionary_from_url};
pub use engine::Engine;
pub use fix::{FixPass, MAX_FIX_PASSES, apply_fixes, fix};
pub use io::{DirEntry, EntryKind, Host, IoError};
pub use net::{UrlPolicyError, validate_dictionary_url};
pub use parse::{ParseError, parse};
pub use position::LineIndex;
