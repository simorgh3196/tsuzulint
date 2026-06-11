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

use tzlint_ast::morphology::Lang;
use tzlint_pdk::RuleId;

pub use discover::{DiscoveredConfig, ShadowedCandidate, discover};
pub use format::ConfigFormat;
pub(crate) use format::reject_yaml_anchors;
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
    /// Optional morphology-dictionary source. `None` (the default) means no tokenizer is wired and
    /// morphology-dependent rules stay inert — and the document cache key is byte-identical to a
    /// pre-morphology run. `Some` makes the CLI provision a hash-pinned dictionary and register a
    /// provider (see [`MorphologyConfig`]).
    pub morphology: Option<MorphologyConfig>,
}

/// A resolved morphology-dictionary source (the validated form of the config `morphology` block).
///
/// The CLI provisions the compressed container from [`source`](MorphologyConfig::source), verifies
/// it against [`pin`](MorphologyConfig::pin), decompresses it in memory, and registers a provider
/// for [`lang`](MorphologyConfig::lang).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphologyConfig {
    /// Where the compressed dictionary container is obtained from.
    pub source: DictSource,
    /// The BLAKE3 pin over the **compressed** container — the value `provision_dictionary` verifies
    /// and the dictionary's cache identity ([`DictId`](crate::DictId)).
    pub pin: [u8; 32],
    /// The language the dictionary serves (currently only `"ja"`).
    pub lang: String,
}

/// Where a dictionary container is obtained: a local file or an https URL. Exactly one is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictSource {
    /// A local path to the compressed `.dict.zst` container (resolved relative to the working dir).
    Path(String),
    /// An https URL to fetch the container from on a cache miss (SSRF-guarded, then pinned).
    Url(String),
}

impl Config {
    /// Parse a config from `text` in the given [`ConfigFormat`].
    ///
    /// Strict: unknown top-level keys, malformed values, or an `extends` naming an unknown
    /// preset are errors. Rule-specific `options` are preserved verbatim as JSON values.
    pub fn parse(text: &str, format: ConfigFormat) -> Result<Self, ConfigError> {
        format::parse(text, format)
    }

    /// The document [`Lang`] this config's [`language`](Config::language) tag resolves to, or
    /// `None` when the tag is unset or unrecognized.
    ///
    /// The primary subtag is matched case-insensitively (`"ja"`, `"JA"`, `"ja-JP"` all map to
    /// [`Lang::JA`]), so an editor- or BCP-47-style tag works. An unknown language is treated as
    /// unset rather than an error: rule scoping then runs only the language-neutral rules, which is
    /// the predictable behavior for untagged text.
    #[must_use]
    pub fn document_lang(&self) -> Option<Lang> {
        let tag = self.language.as_deref()?;
        let primary = tag.split(['-', '_']).next().unwrap_or(tag);
        match primary.to_ascii_lowercase().as_str() {
            "ja" => Some(Lang::JA),
            "ko" => Some(Lang::KO),
            "zh" => Some(Lang::ZH),
            _ => None,
        }
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
    /// A format's `delimiter` override is not an ASCII character. The delimited scanner is
    /// byte-oriented, so a multi-byte delimiter cannot be represented; reject it at parse time.
    NonAsciiDelimiter {
        /// The format id (`csv`/`tsv`) whose delimiter override is non-ASCII.
        format: String,
        /// The offending delimiter character.
        delimiter: char,
    },
    /// The `morphology` section did not set exactly one of `path` / `url`.
    MorphologySource,
    /// The `morphology.pin` is not 64 hexadecimal characters.
    InvalidDictPin(String),
    /// The `morphology.lang` names a language with no supported backend (only `ja` today).
    UnsupportedMorphologyLang(String),
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
            ConfigError::NonAsciiDelimiter { format, delimiter } => {
                write!(
                    f,
                    "delimiter '{delimiter}' in format '{format}' is not ASCII (only single-byte delimiters are supported)"
                )
            }
            ConfigError::MorphologySource => {
                write!(f, "`morphology` requires exactly one of `path` or `url`")
            }
            ConfigError::InvalidDictPin(pin) => {
                write!(
                    f,
                    "`morphology.pin` must be 64 hexadecimal characters: `{pin}`"
                )
            }
            ConfigError::UnsupportedMorphologyLang(lang) => {
                write!(
                    f,
                    "unsupported `morphology.lang` `{lang}` (only `ja` is supported)"
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
        assert_eq!(
            ConfigError::NonAsciiDelimiter {
                format: "csv".into(),
                delimiter: '；',
            }
            .to_string(),
            "delimiter '；' in format 'csv' is not ASCII (only single-byte delimiters are supported)"
        );
        assert_eq!(
            ConfigError::UnknownFormat("xml".into()).to_string(),
            "unknown input format 'xml' (only csv/tsv support columns)"
        );
        assert_eq!(
            ConfigError::ColumnNameWithoutHeader {
                format: "csv".into(),
                name: "body".into(),
            }
            .to_string(),
            "column 'body' in format 'csv' is selected by name but header is false"
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

    #[test]
    fn document_lang_maps_known_tags_and_leaves_the_rest_unset() {
        use tzlint_ast::morphology::Lang;
        let lang = |tag: Option<&str>| {
            Config {
                language: tag.map(str::to_string),
                ..Default::default()
            }
            .document_lang()
        };
        // Unset stays unset (⇒ neutral-only scoping downstream).
        assert_eq!(lang(None), None);
        // Known primary subtags map to their `Lang`, case- and region-insensitively.
        assert_eq!(lang(Some("ja")), Some(Lang::JA));
        assert_eq!(lang(Some("JA")), Some(Lang::JA));
        assert_eq!(lang(Some("ja-JP")), Some(Lang::JA));
        assert_eq!(lang(Some("ko")), Some(Lang::KO));
        assert_eq!(lang(Some("zh-Hans")), Some(Lang::ZH));
        // An unrecognized tag is treated as unset, not an error.
        assert_eq!(lang(Some("en")), None);
        assert_eq!(lang(Some("")), None);
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
