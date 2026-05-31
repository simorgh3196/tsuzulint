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
    // TODO(M1f): populate once `tzlint_rules` defines the rule ids these presets enable.
    fn rules(self) -> BTreeMap<RuleId, RuleSetting> {
        BTreeMap::new()
    }
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
    fn resolve_keeps_user_languages_and_rules() {
        let user = Config {
            language: Some("ja".into()),
            message_language: Some("en".into()),
            rules: {
                let mut m = BTreeMap::new();
                m.insert(RuleId::from("sentence-length"), RuleSetting::Off);
                m
            },
        };
        // Presets are empty in M1, so resolution is currently identity on the rule set.
        let resolved = resolve(Some(Preset::JaBasic), user.clone());
        assert_eq!(resolved, user);
        // No preset behaves the same.
        assert_eq!(resolve(None, user.clone()), user);
    }
}
