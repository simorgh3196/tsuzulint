//! The serde model behind a config file and its conversion to a resolved [`Config`].
//!
//! [`RawConfig`] mirrors the on-disk shape (strict: `deny_unknown_fields`, kebab-case keys);
//! [`RawConfig::into_config`] resolves `extends` presets (layered under this file's `rules`) and
//! lifts string rule keys into [`RuleId`]s. [`RuleSetting`] has a hand-written `Deserialize` so a rule value may be
//! `false`, `true`, or `{ severity?, options? }` while still rejecting unknown keys in the
//! object form — something `#[serde(untagged)]` cannot enforce.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde_json::Value;
use tzlint_pdk::{RuleId, Severity};

use super::{Config, ConfigError, DictSource, MorphologyConfig};

/// The on-disk config shape. Kebab-case keys, no unknown keys.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawConfig {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    message_language: Option<String>,
    #[serde(default)]
    rules: BTreeMap<String, RuleSetting>,
    /// Preset(s) to extend: a base rule layer this file's own `rules` override. A bare string
    /// names one preset; an array names several (later entries win over earlier; this file's
    /// `rules` win over all). `null`/absent means none. Each id must be a known preset (e.g.
    /// `"ja-basic"`); an unknown id is rejected in [`into_config`](RawConfig::into_config). A
    /// non-string/array value is a serde type error at parse time.
    #[serde(default)]
    extends: Option<Extends>,
    /// Per-format sections (`formats.csv` / `formats.tsv`). Resolved into
    /// [`Config::formats`](super::Config::formats); an unknown format id or a name-keyed column
    /// under `header: false` is rejected in [`into_config`](RawConfig::into_config).
    #[serde(default)]
    formats: std::collections::BTreeMap<String, RawFormat>,
    /// Optional morphology-dictionary source for morphology-dependent rules (e.g.
    /// `no-doubled-joshi`). Absent (the default) means no tokenizer is wired and those rules stay
    /// inert. Resolved + validated into [`Config::morphology`](super::Config::morphology).
    #[serde(default)]
    morphology: Option<RawMorphology>,
}

/// The on-disk shape of the `morphology` section. Exactly one of `path`/`url` must be set, and
/// `pin` is the 64-hex BLAKE3 over the COMPRESSED container; both are validated in
/// [`resolve`](RawMorphology::resolve).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawMorphology {
    /// A local path to the compressed `.dict.zst` container.
    #[serde(default)]
    path: Option<String>,
    /// An https URL to fetch the compressed container from (SSRF-guarded).
    #[serde(default)]
    url: Option<String>,
    /// 64 hex characters: the BLAKE3 pin over the compressed container.
    pin: String,
    /// The language this dictionary serves; defaults to `"ja"` (the only one supported today).
    #[serde(default)]
    lang: Option<String>,
}

impl RawMorphology {
    /// Validate the raw section into a resolved [`MorphologyConfig`](super::MorphologyConfig):
    /// exactly one source, a 64-hex pin decoded to 32 bytes, and a supported language.
    fn resolve(self) -> Result<MorphologyConfig, ConfigError> {
        let source = match (self.path, self.url) {
            (Some(path), None) => DictSource::Path(path),
            (None, Some(url)) => DictSource::Url(url),
            _ => return Err(ConfigError::MorphologySource),
        };
        let pin = decode_pin(&self.pin)?;
        let lang = self.lang.unwrap_or_else(|| "ja".to_string());
        if lang != "ja" {
            return Err(ConfigError::UnsupportedMorphologyLang(lang));
        }
        Ok(MorphologyConfig { source, pin, lang })
    }
}

/// Decode a 64-character hex string into a 32-byte pin, panic-free. A wrong length or a non-hex
/// character is a [`ConfigError::InvalidDictPin`].
fn decode_pin(hex: &str) -> Result<[u8; 32], ConfigError> {
    blake3::Hash::from_hex(hex)
        .map(|hash| *hash.as_bytes())
        .map_err(|_| ConfigError::InvalidDictPin(hex.to_string()))
}

/// The on-disk shape of one `formats.<id>` section.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawFormat {
    #[serde(default)]
    header: bool,
    #[serde(default)]
    delimiter: Option<char>,
    #[serde(default)]
    columns: BTreeMap<String, RawColumn>,
}

/// The on-disk shape of one column under `formats.<id>.columns`. The map key is the selector
/// (a 1-based number, or a header name).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawColumn {
    #[serde(default)]
    parse_mode: Option<RawParseMode>,
    #[serde(default)]
    rules: BTreeMap<String, RuleSetting>,
}

/// The on-disk spelling of a cell parse mode.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawParseMode {
    Markdown,
    Plain,
}

/// The shape of the `extends` value: one preset id, or a list of them.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Extends {
    /// `extends: "ja-basic"`.
    One(String),
    /// `extends: ["ja-basic", "ja-technical-writing"]`.
    Many(Vec<String>),
}

impl Extends {
    /// The preset ids in declaration order.
    fn ids(self) -> Vec<String> {
        match self {
            Extends::One(id) => vec![id],
            Extends::Many(ids) => ids,
        }
    }
}

impl RawConfig {
    /// Resolve `extends` presets and lift `rules` keys into [`RuleId`]s.
    ///
    /// The presets form base layers under this file's own `rules` (later `extends` entries win
    /// over earlier; this file's `rules` win over all). An unknown preset id is a
    /// [`ConfigError::UnknownPreset`]. Rule ids inside `rules` are not checked against the
    /// known-rule set here (`deny_unknown_fields` guards only the fixed top-level keys); an
    /// unknown rule id is kept verbatim and simply matches no rule.
    pub(super) fn into_config(self) -> Result<Config, ConfigError> {
        let preset_ids = self.extends.map(Extends::ids).unwrap_or_default();
        let presets = preset_ids
            .into_iter()
            .map(|id| super::Preset::from_id(&id).ok_or(ConfigError::UnknownPreset(id)))
            .collect::<Result<Vec<_>, _>>()?;

        let rules = self
            .rules
            .into_iter()
            .map(|(id, setting)| (RuleId::from(id), setting))
            .collect();
        let user = Config {
            language: self.language,
            message_language: self.message_language,
            rules,
            ..Default::default()
        };

        // Resolve formats (csv/tsv only; columns become selectors + parse mode + rule overlay).
        // Formats are not preset-layered, so they are attached to the FINAL folded config below.
        let mut formats = std::collections::BTreeMap::new();
        for (fmt_id, raw) in self.formats {
            if fmt_id != "csv" && fmt_id != "tsv" {
                return Err(ConfigError::UnknownFormat(fmt_id));
            }
            // The delimited scanner is byte-oriented; a non-ASCII delimiter cannot be represented
            // in one byte and would be silently truncated downstream, so reject it here.
            if let Some(delimiter) = raw.delimiter
                && !delimiter.is_ascii()
            {
                return Err(ConfigError::NonAsciiDelimiter {
                    format: fmt_id,
                    delimiter,
                });
            }
            let mut columns = Vec::new();
            for (key, raw_col) in raw.columns {
                let selector = match key.parse::<u32>() {
                    Ok(n) if n >= 1 => crate::processor::ColumnSelector::Index(n),
                    _ => {
                        if !raw.header {
                            return Err(ConfigError::ColumnNameWithoutHeader {
                                format: fmt_id,
                                name: key,
                            });
                        }
                        crate::processor::ColumnSelector::Name(key)
                    }
                };
                let parse_mode = match raw_col.parse_mode {
                    Some(RawParseMode::Plain) => crate::processor::ParseMode::PlainText,
                    _ => crate::processor::ParseMode::Markdown,
                };
                let rules = raw_col
                    .rules
                    .into_iter()
                    .map(|(id, setting)| (RuleId::from(id), setting))
                    .collect();
                columns.push(crate::ColumnConfig {
                    selector,
                    parse_mode,
                    rules,
                });
            }
            formats.insert(
                fmt_id,
                crate::FormatConfig {
                    has_header: raw.header,
                    delimiter: raw.delimiter,
                    columns,
                },
            );
        }

        // Layer presets under the user config. Folding in reverse makes the precedence (low to
        // high) `extends[0] < … < extends[n] < user`: each `resolve` puts its preset *under* the
        // accumulator.
        let mut result = presets
            .into_iter()
            .rev()
            .fold(user, |acc, preset| super::resolve(Some(preset), acc));
        // Attach the resolved formats to the FINAL folded config (formats are not preset-layered).
        result.formats = formats;
        // Likewise the morphology source: it is this file's own setting (presets never supply one),
        // validated here (exactly one source, a well-formed pin, a supported language).
        result.morphology = self.morphology.map(RawMorphology::resolve).transpose()?;
        Ok(result)
    }
}

/// One rule's setting: disabled, or enabled with an optional severity override and
/// rule-specific options.
///
/// Deserializes from `false` (→ [`Off`](RuleSetting::Off)), `true`
/// (→ [`On`](RuleSetting::On) with no severity override and null options), or an object
/// `{ severity?, options? }`. Unknown keys in the object form are rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSetting {
    /// The rule is turned off.
    Off,
    /// The rule is on, optionally overriding its default severity, with `options` passed
    /// through to the rule (a JSON value; `Null` when omitted).
    On {
        /// Overrides the rule's default severity when `Some`.
        severity: Option<Severity>,
        /// Rule-specific options, preserved verbatim as a JSON value. Values must be
        /// JSON-representable: from YAML, non-string mapping keys are coerced to strings and an
        /// integer outside the i64/u64 range is rejected.
        options: Value,
    },
}

impl RuleSetting {
    /// Whether the rule is enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self, RuleSetting::On { .. })
    }
}

impl<'de> Deserialize<'de> for RuleSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RuleSettingVisitor;

        impl<'de> Visitor<'de> for RuleSettingVisitor {
            type Value = RuleSetting;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("`false`, `true`, or a `{ severity?, options? }` object")
            }

            fn visit_bool<E>(self, enabled: bool) -> Result<RuleSetting, E> {
                Ok(if enabled {
                    RuleSetting::On {
                        severity: None,
                        options: Value::Null,
                    }
                } else {
                    RuleSetting::Off
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<RuleSetting, E>
            where
                E: de::Error,
            {
                // Accept the common boolean spellings as on/off. YAML 1.2 (yaml_serde) parses
                // `yes`/`no`/`on`/`off` as plain strings, not booleans, so without this a natural
                // `rule: no` would be a confusing "invalid type: string" error.
                match value.to_ascii_lowercase().as_str() {
                    "true" | "yes" | "on" => Ok(RuleSetting::On {
                        severity: None,
                        options: Value::Null,
                    }),
                    "false" | "no" | "off" => Ok(RuleSetting::Off),
                    _ => Err(de::Error::invalid_value(de::Unexpected::Str(value), &self)),
                }
            }

            fn visit_map<A>(self, mut map: A) -> Result<RuleSetting, A::Error>
            where
                A: MapAccess<'de>,
            {
                const FIELDS: &[&str] = &["severity", "options"];
                let mut severity: Option<Severity> = None;
                let mut options: Option<Value> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "severity" => {
                            if severity.is_some() {
                                return Err(de::Error::duplicate_field("severity"));
                            }
                            severity = Some(map.next_value::<SeverityRepr>()?.into());
                        }
                        "options" => {
                            if options.is_some() {
                                return Err(de::Error::duplicate_field("options"));
                            }
                            options = Some(map.next_value::<Value>()?);
                        }
                        other => return Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }
                Ok(RuleSetting::On {
                    severity,
                    options: options.unwrap_or(Value::Null),
                })
            }
        }

        // `deserialize_any`: dispatch on the actual JSON/YAML node (bool vs map). Both formats
        // are self-describing, so this is well-defined.
        deserializer.deserialize_any(RuleSettingVisitor)
    }
}

/// Severity as written in config (`"error"`, `"warning"`, `"info"`, `"hint"`), mapped to the
/// PDK [`Severity`]. Kept separate so the PDK stays serde-free (it is `no_std` and ABI-frozen).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SeverityRepr {
    Error,
    Warning,
    Info,
    Hint,
}

impl From<SeverityRepr> for Severity {
    fn from(repr: SeverityRepr) -> Self {
        match repr {
            SeverityRepr::Error => Severity::Error,
            SeverityRepr::Warning => Severity::Warning,
            SeverityRepr::Info => Severity::Info,
            SeverityRepr::Hint => Severity::Hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_setting_false_is_off() {
        let s: RuleSetting = serde_json::from_str("false").unwrap();
        assert_eq!(s, RuleSetting::Off);
        assert!(!s.is_enabled());
    }

    #[test]
    fn rule_setting_true_is_on_with_defaults() {
        let s: RuleSetting = serde_json::from_str("true").unwrap();
        assert_eq!(
            s,
            RuleSetting::On {
                severity: None,
                options: Value::Null,
            }
        );
        assert!(s.is_enabled());
    }

    #[test]
    fn rule_setting_object_severity_and_options() {
        let s: RuleSetting =
            serde_json::from_str(r#"{ "severity": "error", "options": { "max": 3 } }"#).unwrap();
        match s {
            RuleSetting::On { severity, options } => {
                assert_eq!(severity, Some(Severity::Error));
                assert_eq!(options["max"], serde_json::json!(3));
            }
            RuleSetting::Off => panic!("expected On"),
        }
    }

    #[test]
    fn rule_setting_object_partial_fields_default() {
        // Only `options` → severity defaults to None.
        let s: RuleSetting = serde_json::from_str(r#"{ "options": [1, 2] }"#).unwrap();
        assert_eq!(
            s,
            RuleSetting::On {
                severity: None,
                options: serde_json::json!([1, 2]),
            }
        );
        // Empty object → On with all defaults.
        let empty: RuleSetting = serde_json::from_str("{}").unwrap();
        assert_eq!(
            empty,
            RuleSetting::On {
                severity: None,
                options: Value::Null,
            }
        );
    }

    #[test]
    fn rule_setting_accepts_yaml_boolean_spellings() {
        // From YAML, `yes`/`on` and `no`/`off` arrive as strings; map them to On/Off.
        for on in ["yes", "on", "true", "On", "YES"] {
            let s: RuleSetting = yaml_serde::from_str(on).unwrap();
            assert!(s.is_enabled(), "{on} should enable");
        }
        for off in ["no", "off", "false", "Off", "NO"] {
            let s: RuleSetting = yaml_serde::from_str(off).unwrap();
            assert_eq!(s, RuleSetting::Off, "{off} should disable");
        }
        // A non-boolean string is still rejected.
        assert!(yaml_serde::from_str::<RuleSetting>("maybe").is_err());
    }

    #[test]
    fn rule_setting_rejects_unknown_key() {
        let err = serde_json::from_str::<RuleSetting>(r#"{ "severty": "error" }"#).unwrap_err();
        assert!(err.to_string().contains("severty"), "got {err}");
    }

    #[test]
    fn rule_setting_rejects_duplicate_severity() {
        let err =
            serde_json::from_str::<RuleSetting>(r#"{ "severity": "error", "severity": "info" }"#)
                .unwrap_err();
        assert!(err.to_string().contains("severity"), "got {err}");
    }

    #[test]
    fn rule_setting_rejects_duplicate_options() {
        let err =
            serde_json::from_str::<RuleSetting>(r#"{ "options": 1, "options": 2 }"#).unwrap_err();
        assert!(err.to_string().contains("options"), "got {err}");
    }

    #[test]
    fn severity_repr_maps_all_levels() {
        for (text, expected) in [
            ("error", Severity::Error),
            ("warning", Severity::Warning),
            ("info", Severity::Info),
            ("hint", Severity::Hint),
        ] {
            let setting: RuleSetting =
                serde_json::from_str(&format!(r#"{{ "severity": "{text}" }}"#)).unwrap();
            assert_eq!(
                setting,
                RuleSetting::On {
                    severity: Some(expected),
                    options: Value::Null,
                },
                "severity {text}"
            );
        }
    }

    #[test]
    fn rule_setting_rejects_bad_severity_value() {
        let err = serde_json::from_str::<RuleSetting>(r#"{ "severity": "fatal" }"#).unwrap_err();
        assert!(err.to_string().contains("fatal") || err.to_string().contains("variant"));
    }

    #[test]
    fn raw_config_rejects_unknown_top_level_key() {
        let err = serde_json::from_str::<RawConfig>(r#"{ "langauge": "ja" }"#).unwrap_err();
        assert!(err.to_string().contains("langauge"), "got {err}");
    }

    #[test]
    fn into_config_lifts_rule_ids_and_languages() {
        let raw: RawConfig = serde_json::from_str(
            r#"{ "language": "ja", "message-language": "en", "rules": { "sentence-length": false } }"#,
        )
        .unwrap();
        let config = raw.into_config().unwrap();
        assert_eq!(config.language.as_deref(), Some("ja"));
        assert_eq!(config.message_language.as_deref(), Some("en"));
        assert_eq!(
            config.rules.get(&RuleId::from("sentence-length")),
            Some(&RuleSetting::Off)
        );
    }

    /// A 64-character hex pin used across the morphology tests; decodes to bytes `01 23 45 … ef`.
    const PIN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn morphology(json: &str) -> Result<Config, ConfigError> {
        serde_json::from_str::<RawConfig>(json)
            .expect("valid JSON shape")
            .into_config()
    }

    #[test]
    fn morphology_parses_a_local_path_and_decodes_the_pin() {
        let m = morphology(&format!(
            r#"{{ "morphology": {{ "path": "dict/ja.dict.zst", "pin": "{PIN}" }} }}"#
        ))
        .unwrap()
        .morphology
        .expect("morphology present");
        assert_eq!(m.source, DictSource::Path("dict/ja.dict.zst".to_string()));
        assert_eq!(m.lang, "ja"); // defaults to ja
        assert_eq!(m.pin[0], 0x01);
        assert_eq!(m.pin[31], 0xef);
    }

    #[test]
    fn morphology_parses_a_url_source() {
        let m = morphology(&format!(
            r#"{{ "morphology": {{ "url": "https://example.com/ja.dict.zst", "pin": "{PIN}", "lang": "ja" }} }}"#
        ))
        .unwrap()
        .morphology
        .expect("morphology present");
        assert_eq!(
            m.source,
            DictSource::Url("https://example.com/ja.dict.zst".to_string())
        );
    }

    #[test]
    fn morphology_rejects_an_ill_formed_pin() {
        // Too short.
        let err = morphology(r#"{ "morphology": { "path": "d.zst", "pin": "abc" } }"#).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidDictPin(_)), "{err}");
        // Right length, non-hex character ('g').
        let bad = format!("g{}", &PIN[1..]);
        let err = morphology(&format!(
            r#"{{ "morphology": {{ "path": "d.zst", "pin": "{bad}" }} }}"#
        ))
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidDictPin(_)), "{err}");
    }

    #[test]
    fn morphology_rejects_both_or_neither_source() {
        let both = morphology(&format!(
            r#"{{ "morphology": {{ "path": "d.zst", "url": "https://x/d.zst", "pin": "{PIN}" }} }}"#
        ))
        .unwrap_err();
        assert!(matches!(both, ConfigError::MorphologySource), "{both}");
        let neither =
            morphology(&format!(r#"{{ "morphology": {{ "pin": "{PIN}" }} }}"#)).unwrap_err();
        assert!(
            matches!(neither, ConfigError::MorphologySource),
            "{neither}"
        );
    }

    #[test]
    fn morphology_rejects_an_unsupported_language() {
        let err = morphology(&format!(
            r#"{{ "morphology": {{ "path": "d.zst", "pin": "{PIN}", "lang": "ko" }} }}"#
        ))
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::UnsupportedMorphologyLang(ref l) if l == "ko"),
            "{err}"
        );
    }

    #[test]
    fn config_without_morphology_resolves_to_none() {
        // The load-bearing invariant: no `morphology` key ⇒ `None` ⇒ an empty registry ⇒ a
        // byte-identical pre-morphology cache key.
        assert!(
            morphology(r#"{ "rules": {} }"#)
                .unwrap()
                .morphology
                .is_none()
        );
    }

    #[test]
    fn morphology_config_errors_render_their_messages() {
        assert!(
            ConfigError::MorphologySource
                .to_string()
                .contains("exactly one")
        );
        assert!(
            ConfigError::InvalidDictPin("abc".to_string())
                .to_string()
                .contains("64 hexadecimal")
        );
        assert!(
            ConfigError::UnsupportedMorphologyLang("ko".to_string())
                .to_string()
                .contains("ko")
        );
    }

    #[test]
    fn morphology_rejects_unknown_keys() {
        // `deny_unknown_fields` guards the section against typos.
        let err = serde_json::from_str::<RawConfig>(&format!(
            r#"{{ "morphology": {{ "path": "d.zst", "pin": "{PIN}", "compress": true }} }}"#
        ));
        assert!(err.is_err(), "unknown morphology key must be rejected");
    }

    #[test]
    fn into_config_resolves_extends_under_user_rules() {
        // `extends` contributes the preset's rules as a base; this file's `rules` win on overlap.
        let raw: RawConfig = serde_json::from_str(
            r#"{ "extends": "ja-basic", "rules": { "no-hankaku-kana": false } }"#,
        )
        .unwrap();
        let config = raw.into_config().unwrap();
        // ja-basic enables no-mixed-zenkaku-hankaku-alphabet (kept) ...
        assert!(
            config
                .rules
                .get(&RuleId::from("no-mixed-zenkaku-hankaku-alphabet"))
                .is_some_and(RuleSetting::is_enabled)
        );
        // ... and no-hankaku-kana, which this file overrides to off (user wins).
        assert_eq!(
            config.rules.get(&RuleId::from("no-hankaku-kana")),
            Some(&RuleSetting::Off)
        );
    }

    #[test]
    fn into_config_array_extends_merges_all_presets() {
        // An array `extends` layers every listed preset's rules. Later entries would win on a
        // shared id, but the two built-ins never set a shared rule differently, so the only
        // observable effect here is the union: ja-basic's rules plus ja-technical-writing's extras.
        let raw: RawConfig =
            serde_json::from_str(r#"{ "extends": ["ja-basic", "ja-technical-writing"] }"#).unwrap();
        let config = raw.into_config().unwrap();
        // Contributed by ja-basic (the earlier entry).
        assert!(config.rules.contains_key(&RuleId::from("no-hankaku-kana")));
        // Unique to ja-technical-writing (the later entry).
        assert!(config.rules.contains_key(&RuleId::from("sentence-length")));
        assert!(config.rules.contains_key(&RuleId::from("max-ten")));
    }

    #[test]
    fn into_config_rejects_unknown_preset() {
        let raw: RawConfig = serde_json::from_str(r#"{ "extends": "nope" }"#).unwrap();
        assert!(matches!(
            raw.into_config(),
            Err(ConfigError::UnknownPreset(id)) if id == "nope"
        ));
    }

    #[test]
    fn into_config_null_extends_is_a_noop() {
        let raw: RawConfig = serde_json::from_str(r#"{ "extends": null }"#).unwrap();
        assert!(raw.into_config().unwrap().rules.is_empty());
    }
}

#[cfg(test)]
mod formats_parsing {
    use crate::processor::{ColumnSelector, ParseMode};
    use crate::{Config, ConfigError, ConfigFormat, RuleSetting};

    fn parse(json: &str) -> Result<Config, ConfigError> {
        Config::parse(json, ConfigFormat::Json)
    }

    #[test]
    fn parses_columns_by_name_and_index_with_overlay_rules() {
        let c = parse(
            r#"{ "formats": { "csv": { "header": true,
                "columns": {
                  "body": { "rules": { "no-todo": true } },
                  "2": { "parse-mode": "plain", "rules": { "max-ten": { "options": { "max": 0 } } } }
                } } } }"#,
        )
        .unwrap();
        let csv = c.formats.get("csv").unwrap();
        assert!(csv.has_header);
        assert_eq!(csv.columns.len(), 2);
        // BTreeMap key order: "2" then "body" — assert by finding.
        let body = csv
            .columns
            .iter()
            .find(|col| col.selector == ColumnSelector::Name("body".into()))
            .unwrap();
        assert_eq!(body.parse_mode, ParseMode::Markdown);
        assert!(matches!(
            body.rules.get(&"no-todo".into()),
            Some(RuleSetting::On { .. })
        ));
        let col2 = csv
            .columns
            .iter()
            .find(|col| col.selector == ColumnSelector::Index(2))
            .unwrap();
        assert_eq!(col2.parse_mode, ParseMode::PlainText);
    }

    #[test]
    fn name_key_without_header_is_an_error() {
        let err =
            parse(r#"{ "formats": { "tsv": { "header": false, "columns": { "body": {} } } } }"#)
                .unwrap_err();
        assert!(
            matches!(err, ConfigError::ColumnNameWithoutHeader { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_format_id_is_an_error() {
        let err = parse(r#"{ "formats": { "xml": { "columns": { "1": {} } } } }"#).unwrap_err();
        assert!(
            matches!(err, ConfigError::UnknownFormat(ref f) if f == "xml"),
            "{err:?}"
        );
    }

    #[test]
    fn non_ascii_delimiter_is_an_error() {
        // The scanner is byte-oriented, so a non-ASCII delimiter would truncate via `char as u8`
        // and corrupt extraction. Reject it at parse time instead.
        let err = parse(
            r#"{ "formats": { "csv": { "header": true, "delimiter": "；", "columns": { "1": {} } } } }"#,
        )
        .unwrap_err();
        assert!(
            matches!(err, ConfigError::NonAsciiDelimiter { ref format, delimiter } if format == "csv" && delimiter == '；'),
            "{err:?}"
        );
    }

    #[test]
    fn ascii_delimiter_override_is_accepted() {
        // An ASCII override (e.g. a semicolon) is fine — it fits in one byte.
        let c = parse(
            r#"{ "formats": { "csv": { "header": true, "delimiter": ";", "columns": { "1": {} } } } }"#,
        )
        .unwrap();
        assert_eq!(c.formats.get("csv").unwrap().delimiter, Some(';'));
    }
}
