//! The serde model behind a config file and its conversion to a resolved [`Config`].
//!
//! [`RawConfig`] mirrors the on-disk shape (strict: `deny_unknown_fields`, kebab-case keys);
//! [`RawConfig::into_config`] validates the reserved `extends` key and lifts string rule keys
//! into [`RuleId`]s. [`RuleSetting`] has a hand-written `Deserialize` so a rule value may be
//! `false`, `true`, or `{ severity?, options? }` while still rejecting unknown keys in the
//! object form — something `#[serde(untagged)]` cannot enforce.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde_json::Value;
use tzlint_pdk::{RuleId, Severity};

use super::{Config, ConfigError};

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
    /// Reserved: composing/extending configs is a later milestone. Accepted into the model (so
    /// a lone `extends` produces a precise "reserved" error rather than an "unknown field" one)
    /// and rejected in [`into_config`](RawConfig::into_config). `extends: null` is treated as
    /// absent (a no-op), matching serde's null→`None` mapping. Note: if the file *also* has an
    /// unknown key or a type error, serde's `deny_unknown_fields`/type error fires first and
    /// that message wins over the reserved-key one.
    #[serde(default)]
    extends: Option<Value>,
}

impl RawConfig {
    /// Validate reserved keys and lift `rules` keys into [`RuleId`]s.
    ///
    /// Rule ids are not checked against a known-rule set here — that registry does not exist
    /// until the rules crate lands (M1f), so an unknown id is kept verbatim and simply matches
    /// no rule. (`deny_unknown_fields` guards the fixed top-level keys, not the dynamic `rules`
    /// map keys.)
    pub(super) fn into_config(self) -> Result<Config, ConfigError> {
        if self.extends.is_some() {
            return Err(ConfigError::Reserved("extends"));
        }
        let rules = self
            .rules
            .into_iter()
            .map(|(id, setting)| (RuleId::from(id), setting))
            .collect();
        Ok(Config {
            language: self.language,
            message_language: self.message_language,
            rules,
        })
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

    #[test]
    fn into_config_rejects_reserved_extends() {
        let raw: RawConfig = serde_json::from_str(r#"{ "extends": ["ja-basic"] }"#).unwrap();
        assert!(matches!(
            raw.into_config(),
            Err(ConfigError::Reserved("extends"))
        ));
        // `extends: null` is treated as absent (a no-op), not reserved.
        let null_extends: RawConfig = serde_json::from_str(r#"{ "extends": null }"#).unwrap();
        assert!(null_extends.into_config().is_ok());
    }
}
