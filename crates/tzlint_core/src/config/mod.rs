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
//!   overrides by id. A config selects them with `extends` (a preset id or a list of them);
//!   they are layered under the file's own `rules`, and an unknown id is a clear error.

mod discover;
mod format;
mod format_config;
mod model;
mod preset;
mod schema;

use std::collections::BTreeMap;
use std::fmt;

use tzlint_pdk::RuleId;

pub use discover::{DiscoveredConfig, ShadowedCandidate, discover};
pub use format::ConfigFormat;
pub use format_config::{ColumnConfig, FormatConfig};
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
    /// Per-format settings (e.g. `formats.csv`), keyed by format id. Empty by default.
    pub formats: BTreeMap<String, FormatConfig>,
}

impl Config {
    /// Parse a config from `text` in the given [`ConfigFormat`].
    ///
    /// Strict: unknown top-level keys, malformed values, or an `extends` naming an unknown
    /// preset are errors. Rule-specific `options` are preserved verbatim as JSON values.
    pub fn parse(text: &str, format: ConfigFormat) -> Result<Self, ConfigError> {
        format::parse(text, format)
    }
}

/// A configuration failure: an I/O error, a parse/validation error, or an unknown preset id.
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
    /// `extends` referenced a preset id that is not a known [`Preset`].
    UnknownPreset(String),
    /// A `formats` key that is not a known input format (only `csv`/`tsv` support columns).
    UnknownFormat(String),
    /// A column was selected by name under a format with `header: false`.
    ColumnNameWithoutHeader {
        /// The format id (`csv`/`tsv`) whose column was selected by name.
        format: String,
        /// The header name that cannot be resolved without a header row.
        name: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {e}"),
            ConfigError::Parse { format, message } => {
                write!(f, "failed to parse {format} config: {message}")
            }
            ConfigError::UnknownPreset(id) => {
                write!(f, "`extends` references unknown preset `{id}`")
            }
            ConfigError::UnknownFormat(format) => {
                write!(
                    f,
                    "unknown input format '{format}' (only csv/tsv support columns)"
                )
            }
            ConfigError::ColumnNameWithoutHeader { format, name } => {
                write!(
                    f,
                    "column '{name}' in format '{format}' is selected by name but header is false"
                )
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
            ConfigError::UnknownPreset("nope".into()).to_string(),
            "`extends` references unknown preset `nope`"
        );
    }

    #[test]
    fn error_source_chains_io_only() {
        use std::error::Error;
        assert!(ConfigError::Io(IoError::NotFound).source().is_some());
        assert!(ConfigError::UnknownPreset("nope".into()).source().is_none());
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

#[cfg(test)]
mod format_config_tests {
    use super::*;

    #[test]
    fn config_default_has_no_formats() {
        let c = Config::default();
        assert!(c.formats.is_empty());
    }
}
