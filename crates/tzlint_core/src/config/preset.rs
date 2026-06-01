//! Presets: named base rule sets a user config layers over.

use std::collections::BTreeMap;

use tzlint_pdk::RuleId;

use super::{Config, RuleSetting};

/// A built-in preset: a base set of rule settings the user config overrides by id.
///
/// The concrete rule sets are populated when `tzlint_rules` lands (M1f); the resolution
/// machinery ([`resolve`]) is already complete and tested, so filling them in is additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// A small, broadly-applicable base for Japanese prose.
    JaBasic,
    /// The stricter Japanese technical-writing rule set.
    JaTechnicalWriting,
}

impl Preset {
    /// The preset's stable string id (as used in config/CLI).
    pub fn id(self) -> &'static str {
        match self {
            Preset::JaBasic => "ja-basic",
            Preset::JaTechnicalWriting => "ja-technical-writing",
        }
    }

    /// Parse a preset id, or `None` if unrecognized.
    pub fn from_id(id: &str) -> Option<Preset> {
        match id {
            "ja-basic" => Some(Preset::JaBasic),
            "ja-technical-writing" => Some(Preset::JaTechnicalWriting),
            _ => None,
        }
    }

    /// The rule settings this preset contributes as a base layer.
    ///
    /// Rule ids are referenced as strings (no dependency on `tzlint_rules`); they MUST match the
    /// ids in `tzlint_rules::RULE_IDS` verbatim. Rules whose detection needs morphology (M2),
    /// such as `no-doubled-joshi`, are intentionally omitted until that lands — additively.
    /// Options here are config-layer metadata; routing them into rule instances is a later step,
    /// so a rule currently runs with its own defaults regardless of the value set here.
    fn rules(self) -> BTreeMap<RuleId, RuleSetting> {
        match self {
            Preset::JaBasic => ja_basic(),
            Preset::JaTechnicalWriting => ja_technical_writing(),
        }
    }
}

/// Enable a rule with the given options and no severity override.
fn on(options: serde_json::Value) -> RuleSetting {
    RuleSetting::On {
        severity: None,
        options,
    }
}

/// The `ja-basic` preset: a small, broadly-applicable base.
fn ja_basic() -> BTreeMap<RuleId, RuleSetting> {
    use serde_json::Value;
    [
        ("no-hankaku-kana", on(Value::Null)),
        ("no-mixed-zenkaku-hankaku-alphabet", on(Value::Null)),
        ("no-nfd", on(Value::Null)),
        ("no-zero-width-spaces", on(Value::Null)),
        ("ja-no-mixed-period", on(Value::Null)),
    ]
    .into_iter()
    .map(|(id, setting)| (RuleId::from(id), setting))
    .collect()
}

/// The `ja-technical-writing` preset: stricter, with thresholds.
fn ja_technical_writing() -> BTreeMap<RuleId, RuleSetting> {
    use serde_json::{Value, json};
    [
        ("sentence-length", on(json!({ "max": 100 }))),
        ("max-ten", on(json!({ "max": 3 }))),
        ("max-kanji-continuous-len", on(json!({ "max": 6 }))),
        ("no-hankaku-kana", on(Value::Null)),
        ("no-mixed-zenkaku-hankaku-alphabet", on(Value::Null)),
        ("no-nfd", on(Value::Null)),
        ("no-zero-width-spaces", on(Value::Null)),
        ("no-exclamation-question-mark", on(Value::Null)),
        ("ja-no-mixed-period", on(Value::Null)),
    ]
    .into_iter()
    .map(|(id, setting)| (RuleId::from(id), setting))
    .collect()
}

/// Resolve a `user` config over an optional `preset` base.
///
/// The preset's rules form the base layer; the user's rules override by id (user wins on a
/// collision). `language`/`message_language` come from the user config — presets do not set
/// them in M1.
pub fn resolve(preset: Option<Preset>, user: Config) -> Config {
    let base = preset.map(Preset::rules).unwrap_or_default();
    Config {
        language: user.language,
        message_language: user.message_language,
        rules: merge(base, user.rules),
    }
}

/// Overlay `user` rules onto `base` rules; on a shared id, `user` wins.
fn merge(
    mut base: BTreeMap<RuleId, RuleSetting>,
    user: BTreeMap<RuleId, RuleSetting>,
) -> BTreeMap<RuleId, RuleSetting> {
    base.extend(user);
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn on() -> RuleSetting {
        RuleSetting::On {
            severity: None,
            options: Value::Null,
        }
    }

    #[test]
    fn preset_id_roundtrips() {
        for p in [Preset::JaBasic, Preset::JaTechnicalWriting] {
            assert_eq!(Preset::from_id(p.id()), Some(p));
        }
        assert_eq!(Preset::from_id("nope"), None);
    }

    #[test]
    fn merge_user_overrides_base_by_id() {
        let mut base = BTreeMap::new();
        base.insert(RuleId::from("shared"), RuleSetting::Off);
        base.insert(RuleId::from("base-only"), on());
        let mut user = BTreeMap::new();
        user.insert(RuleId::from("shared"), on()); // user re-enables a base-disabled rule
        user.insert(RuleId::from("user-only"), RuleSetting::Off);

        let merged = merge(base, user);
        assert_eq!(merged.get(&RuleId::from("shared")), Some(&on()));
        assert_eq!(merged.get(&RuleId::from("base-only")), Some(&on()));
        assert_eq!(
            merged.get(&RuleId::from("user-only")),
            Some(&RuleSetting::Off)
        );
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn resolve_layers_preset_under_user() {
        let user = Config {
            language: Some("ja".into()),
            message_language: Some("en".into()),
            rules: {
                let mut m = BTreeMap::new();
                m.insert(RuleId::from("no-hankaku-kana"), RuleSetting::Off); // override a preset rule
                m.insert(RuleId::from("custom-rule"), on()); // user-only
                m
            },
        };
        let resolved = resolve(Some(Preset::JaBasic), user.clone());
        // Languages come from the user.
        assert_eq!(resolved.language.as_deref(), Some("ja"));
        assert_eq!(resolved.message_language.as_deref(), Some("en"));
        // The user override wins on a shared id.
        assert_eq!(
            resolved.rules.get(&RuleId::from("no-hankaku-kana")),
            Some(&RuleSetting::Off)
        );
        // The preset's other rule is present as the base layer; the user-only rule is kept.
        assert!(
            resolved
                .rules
                .contains_key(&RuleId::from("no-mixed-zenkaku-hankaku-alphabet"))
        );
        assert!(resolved.rules.contains_key(&RuleId::from("custom-rule")));
        // No preset → identity.
        assert_eq!(resolve(None, user.clone()), user);
    }

    #[test]
    fn presets_are_populated_with_kebab_ids() {
        for preset in [Preset::JaBasic, Preset::JaTechnicalWriting] {
            let rules = preset.rules();
            assert!(!rules.is_empty(), "{} should have rules", preset.id());
            for id in rules.keys() {
                assert!(
                    id.as_str()
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                    "non-kebab rule id: {}",
                    id.as_str()
                );
            }
        }
        // ja-technical-writing carries thresholds (sentence-length); ja-basic does not.
        assert!(
            Preset::JaTechnicalWriting
                .rules()
                .contains_key(&RuleId::from("sentence-length"))
        );
        assert!(
            !Preset::JaBasic
                .rules()
                .contains_key(&RuleId::from("sentence-length"))
        );
    }
}
