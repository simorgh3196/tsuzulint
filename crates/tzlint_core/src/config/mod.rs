//! Multi-format configuration: discovery, parsing, strict validation, and preset resolution.
//!
//! A TsuzuLint config selects a document [`language`](Config::language), a
//! [`message_language`](Config::message_language) (the diagnostic locale, independent of the
//! document language), and a per-rule [`rules`](Config::rules) map. Three concerns live here:
//!
//! - **Discovery** ([`discover`]) walks upward from a start directory. The first directory
//!   that holds any candidate file wins; within it the highest-priority candidate is loaded
//!   and any co-located lower-priority candidates are reported as [`warnings`](DiscoveredConfig::warnings)
//!   (they are ignored, not merged). Presence is probed through the [`Host`](crate::io::Host)
//!   boundary, so discovery works on native and `wasm32` alike.
//! - **Parsing** ([`Config::parse`]) is strict: unknown keys are an error
//!   (`deny_unknown_fields`). Formats, highest priority first:
//!   `.tzlintrc.jsonc` → `.tzlintrc.json` → `.tzlintrc.yaml` → `.tzlintrc.yml` → `.tzlintrc`
//!   (the extensionless file is parsed as JSONC).
//! - **Presets** ([`Preset`], [`resolve`]) provide a base rule set that the user config
//!   overrides by id. The `extends` key is reserved for a later milestone and is currently
//!   rejected with a clear error rather than silently ignored.

mod discover;
mod format;
mod model;
mod preset;
mod schema;

use std::collections::BTreeMap;
use std::fmt;

use tzlint_pdk::RuleId;

pub use discover::{DiscoveredConfig, ShadowedCandidate, discover};
pub use format::ConfigFormat;
pub use model::RuleSetting;
pub use preset::{Preset, resolve};
pub use schema::CONFIG_SCHEMA;

use crate::io::IoError;

/// A resolved configuration: the document/message languages and the per-rule settings map.
///
/// Build one with [`Config::parse`] (from a single file's text), [`discover`] (walk-up
/// discovery), or [`resolve`] (layer a user config over a preset). [`Config::default`] is the
/// empty config (no language pins, no rule overrides).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// The document language (e.g. `"ja"`), or `None` to let the embedder decide.
    pub language: Option<String>,
    /// The diagnostic-message locale, independent of [`language`](Config::language).
    pub message_language: Option<String>,
    /// Per-rule settings, keyed by [`RuleId`]. Absent ids fall back to a rule's own defaults.
    pub rules: BTreeMap<RuleId, RuleSetting>,
}

impl Config {
    /// Parse a config from `text` in the given [`ConfigFormat`].
    ///
    /// Strict: unknown top-level keys, malformed values, or a (reserved) `extends` key are
    /// errors. Rule-specific `options` are preserved verbatim as JSON values.
    pub fn parse(text: &str, format: ConfigFormat) -> Result<Self, ConfigError> {
        format::parse(text, format)
    }
}

/// A configuration failure: an I/O error, a parse/validation error, or use of a reserved key.
#[derive(Debug)]
pub enum ConfigError {
    /// Reading a discovered config file failed.
    Io(IoError),
    /// The file did not parse or violated the schema (unknown key, bad value, …).
    Parse {
        /// The format the text was parsed as.
        format: ConfigFormat,
        /// A human-readable reason from the underlying deserializer.
        message: String,
    },
    /// A key that is recognized but reserved for a future milestone (e.g. `extends`).
    Reserved(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {e}"),
            ConfigError::Parse { format, message } => {
                write!(f, "failed to parse {format} config: {message}")
            }
            ConfigError::Reserved(key) => {
                write!(f, "`{key}` is reserved and not supported yet")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<IoError> for ConfigError {
    fn from(error: IoError) -> Self {
        ConfigError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        assert_eq!(
            ConfigError::Io(IoError::NotFound).to_string(),
            "config I/O error: file not found"
        );
        assert_eq!(
            ConfigError::Parse {
                format: ConfigFormat::Json,
                message: "boom".into(),
            }
            .to_string(),
            "failed to parse JSON config: boom"
        );
        assert_eq!(
            ConfigError::Reserved("extends").to_string(),
            "`extends` is reserved and not supported yet"
        );
    }

    #[test]
    fn error_source_chains_io_only() {
        use std::error::Error;
        assert!(ConfigError::Io(IoError::NotFound).source().is_some());
        assert!(ConfigError::Reserved("extends").source().is_none());
    }

    #[test]
    fn default_config_is_empty() {
        let c = Config::default();
        assert!(c.language.is_none());
        assert!(c.message_language.is_none());
        assert!(c.rules.is_empty());
    }

    #[test]
    fn config_parse_delegates_to_format() {
        let c = Config::parse(r#"{ "language": "ja" }"#, ConfigFormat::Json).unwrap();
        assert_eq!(c.language.as_deref(), Some("ja"));
        assert!(matches!(
            Config::parse("{ not json", ConfigFormat::Json),
            Err(ConfigError::Parse { .. })
        ));
    }
}
