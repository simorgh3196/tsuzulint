//! `tzlint_core` — parser, lint engine, config, cache, and centralized I/O.
//!
//! Compiles for native and `wasm32`. Houses:
//! - the markdown-rs parser + mdast → index-AST transform,
//! - the format-neutral `processor` seam (`Processor`/`Registry`/`lint_document`) that selects a
//!   parser by file extension (defaulting to Markdown) — `lint_document` is the single
//!   document-level dispatch entry point,
//! - the single-traversal multi-visitor `Engine::lint`, the AST-level executor `lint_document`
//!   runs over each already-parsed region,
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
pub mod morphology;
pub mod net;
pub mod parse;
pub mod position;
pub mod processor;

pub use cache::{
    CacheError, CacheKey, CacheKeyInput, DocumentCache, document_cache_key, lint_cached,
};
pub use config::{
    CONFIG_SCHEMA, ColumnConfig, Config, ConfigError, ConfigFormat, DictSource, DiscoveredConfig,
    FormatConfig, MorphologyConfig, Preset, RuleSetting, ShadowedCandidate, discover, resolve,
};
pub use dict::container::{ContainerError, DictContainer, Member};
pub use dict::{
    DictError, decompress_dictionary, provision_dictionary, provision_dictionary_from_url,
};
pub use engine::Engine;
pub use fix::{FixPass, MAX_FIX_PASSES, apply_fixes, fix};
pub use io::{DirEntry, EntryKind, Host, IoError};
pub use morphology::{DictId, MorphologyRegistry};
pub use net::{UrlPolicyError, validate_dictionary_url};
pub use parse::{ParseError, parse};
pub use position::LineIndex;
pub use processor::{
    ColumnSelector, ColumnTarget, DelimitedConfig, DelimitedProcessor, ParseMode, Parsed,
    Processor, ProcessorConfig, Region, RegionRules, RegionTag, Registry, lint_document,
};

#[cfg(test)]
mod tests {
    use crate::{ProcessorConfig, RegionRules, Registry, lint_document};

    #[test]
    fn processor_seam_is_reexported_at_crate_root() {
        let reg = Registry::with_builtins();
        let diags = lint_document(
            Some("md"),
            "x\n",
            &reg,
            &ProcessorConfig::default(),
            &RegionRules::base_only(vec![]),
            None,
        )
        .unwrap();
        assert!(diags.is_empty());
    }
}
