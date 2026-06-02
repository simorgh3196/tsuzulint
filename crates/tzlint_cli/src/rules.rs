//! Resolving a [`Config`] to the set of [`Rule`]s the engine should run.
//!
//! Activation model: **every built-in rule is on by default**; a `config.rules` entry set to
//! `false` (→ [`RuleSetting::Off`]) disables that rule. So a bare `tzlint lint` runs the full
//! built-in set, and a config narrows it.
//!
//! For an enabled rule, the config's per-rule `options` and optional severity override are
//! routed into construction through [`tzlint_rules::build_rule`] (which applies each rule's
//! `from_options` and wraps it for a severity override). A rule absent from `config.rules` runs
//! with default options and its default severity. Unknown rule ids in config are surfaced via
//! [`unknown_rule_ids`] so a typo'd setting is not silently ignored.

use std::collections::BTreeSet;

use serde_json::Value;
use tzlint_core::{Config, RuleSetting};
use tzlint_pdk::{Rule, RuleId, Severity};
use tzlint_rules::{RULE_IDS, build_rule};

/// Build the boxed rule set to run for `config`: every built-in rule (in [`RULE_IDS`] order)
/// except those a `config.rules` entry turns off, each constructed with its configured options
/// and severity.
#[must_use]
pub fn resolve_rules(config: &Config) -> Vec<Box<dyn Rule>> {
    RULE_IDS
        .iter()
        .filter_map(|id| match config.rules.get(&RuleId::from(*id)) {
            Some(RuleSetting::Off) => None,
            Some(RuleSetting::On { severity, options }) => build_rule(id, options, *severity),
            None => build_rule(id, &Value::Null, None),
        })
        .collect()
}

/// The effective state of one built-in rule under a resolved [`Config`] — what the `rules`
/// subcommand reports. `severity` is the override when the config sets one (with
/// `severity_overridden` true), else the rule's default. `options` holds the config-supplied
/// options only when non-null.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInfo {
    /// The rule id (a built-in, from [`RULE_IDS`]).
    pub id: &'static str,
    /// Whether the rule runs (false only when `config.rules` turns it off).
    pub enabled: bool,
    /// The effective severity: the config override if set, otherwise the rule's default.
    pub severity: Severity,
    /// Whether `severity` came from a config override rather than the rule default.
    pub severity_overridden: bool,
    /// The config-supplied options for this rule, if it set any (non-null).
    pub options: Option<Value>,
}

/// The effective [`RuleInfo`] for every built-in rule, in [`RULE_IDS`] order.
#[must_use]
pub fn rule_infos(config: &Config) -> Vec<RuleInfo> {
    RULE_IDS
        .iter()
        .map(|id| rule_info_for(config, id))
        .collect()
}

/// The [`RuleInfo`] for one rule id, or `None` if it is not a built-in rule.
#[must_use]
pub fn rule_info(config: &Config, id: &str) -> Option<RuleInfo> {
    RULE_IDS
        .iter()
        .find(|known| **known == id)
        .map(|known| rule_info_for(config, known))
}

/// Compute the effective state of `id` under `config`. `id` must be a built-in rule id (a
/// `&'static str` from [`RULE_IDS`]) so the resulting [`RuleInfo`] can borrow it.
fn rule_info_for(config: &Config, id: &'static str) -> RuleInfo {
    let setting = config.rules.get(&RuleId::from(id));
    let enabled = !matches!(setting, Some(RuleSetting::Off));
    let (override_severity, options) = match setting {
        Some(RuleSetting::On { severity, options }) => {
            let options = (!options.is_null()).then(|| options.clone());
            (*severity, options)
        }
        _ => (None, None),
    };
    // The default severity comes from the constructed rule's meta; every RULE_IDS entry builds,
    // so the fallback is unreachable (avoids an unwrap in deny-unwrap code).
    let default_severity = build_rule(id, &Value::Null, None)
        .map_or(Severity::Warning, |rule| rule.meta().default_severity);
    RuleInfo {
        id,
        enabled,
        severity: override_severity.unwrap_or(default_severity),
        severity_overridden: override_severity.is_some(),
        options,
    }
}

/// The rule ids referenced in `config.rules` that are not built-in rules — most likely typos.
/// The CLI reports these so a misspelled setting is not silently ignored.
#[must_use]
pub fn unknown_rule_ids(config: &Config) -> Vec<String> {
    let known: BTreeSet<&str> = RULE_IDS.iter().copied().collect();
    config
        .rules
        .keys()
        .map(|id| id.as_str())
        .filter(|id| !known.contains(id))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use serde_json::Value;
    use tzlint_pdk::RuleId;

    fn config_with(rules: &[(&str, RuleSetting)]) -> Config {
        let mut map = BTreeMap::new();
        for (id, setting) in rules {
            map.insert(RuleId::from(*id), setting.clone());
        }
        Config {
            language: None,
            message_language: None,
            rules: map,
        }
    }

    fn ids(rules: &[Box<dyn Rule>]) -> Vec<String> {
        rules
            .iter()
            .map(|r| r.meta().id.as_str().to_string())
            .collect()
    }

    #[test]
    fn default_config_runs_every_built_in_rule() {
        let rules = resolve_rules(&Config::default());
        assert_eq!(rules.len(), RULE_IDS.len());
        assert_eq!(ids(&rules), RULE_IDS);
    }

    #[test]
    fn off_setting_disables_only_that_rule() {
        let target = RULE_IDS[0];
        let config = config_with(&[(target, RuleSetting::Off)]);
        let remaining = ids(&resolve_rules(&config));
        assert_eq!(remaining.len(), RULE_IDS.len() - 1);
        assert!(
            !remaining.contains(&target.to_string()),
            "{target} should be disabled"
        );
    }

    #[test]
    fn on_setting_keeps_the_rule() {
        let target = RULE_IDS[0];
        let config = config_with(&[(
            target,
            RuleSetting::On {
                severity: None,
                options: Value::Null,
            },
        )]);
        assert!(ids(&resolve_rules(&config)).contains(&target.to_string()));
    }

    #[test]
    fn on_setting_applies_severity_override() {
        use tzlint_pdk::Severity;
        let target = "max-ten";
        let config = config_with(&[(
            target,
            RuleSetting::On {
                severity: Some(Severity::Error),
                options: Value::Null,
            },
        )]);
        let rules = resolve_rules(&config);
        let rule = rules
            .iter()
            .find(|r| r.meta().id.as_str() == target)
            .expect("max-ten should be enabled");
        assert_eq!(rule.meta().default_severity, Severity::Error);
    }

    #[test]
    fn unknown_rule_ids_are_reported() {
        let config = config_with(&[
            ("definitely-not-a-rule", RuleSetting::Off),
            (RULE_IDS[0], RuleSetting::Off),
        ]);
        assert_eq!(unknown_rule_ids(&config), vec!["definitely-not-a-rule"]);
    }

    #[test]
    fn known_rule_ids_are_not_reported() {
        let config = config_with(&[(RULE_IDS[0], RuleSetting::Off)]);
        assert!(unknown_rule_ids(&config).is_empty());
    }

    #[test]
    fn rule_infos_lists_all_in_order_enabled_by_default() {
        let infos = rule_infos(&Config::default());
        assert_eq!(infos.len(), RULE_IDS.len());
        assert_eq!(infos.iter().map(|i| i.id).collect::<Vec<_>>(), RULE_IDS);
        assert!(
            infos
                .iter()
                .all(|i| i.enabled && !i.severity_overridden && i.options.is_none()),
            "default config: every rule enabled, default severity, no options"
        );
    }

    #[test]
    fn rule_infos_marks_a_disabled_rule() {
        let target = RULE_IDS[0];
        let infos = rule_infos(&config_with(&[(target, RuleSetting::Off)]));
        let info = infos.iter().find(|i| i.id == target).unwrap();
        assert!(!info.enabled);
        // The others stay enabled.
        assert_eq!(
            infos.iter().filter(|i| i.enabled).count(),
            RULE_IDS.len() - 1
        );
    }

    #[test]
    fn rule_infos_applies_and_flags_a_severity_override() {
        let target = "max-ten";
        let infos = rule_infos(&config_with(&[(
            target,
            RuleSetting::On {
                severity: Some(Severity::Error),
                options: Value::Null,
            },
        )]));
        let info = infos.iter().find(|i| i.id == target).unwrap();
        assert!(info.enabled);
        assert_eq!(info.severity, Severity::Error);
        assert!(info.severity_overridden);
    }

    #[test]
    fn rule_info_captures_config_options_without_overriding_severity() {
        let target = "max-ten";
        let options = serde_json::json!({ "max": 0 });
        let config = config_with(&[(
            target,
            RuleSetting::On {
                severity: None,
                options: options.clone(),
            },
        )]);
        let info = rule_info(&config, target).unwrap();
        assert_eq!(info.options.as_ref(), Some(&options));
        assert!(!info.severity_overridden);
    }

    #[test]
    fn rule_info_unknown_is_none() {
        assert!(rule_info(&Config::default(), "definitely-not-a-rule").is_none());
    }

    #[test]
    fn rule_info_default_severity_matches_the_built_rule() {
        // The reported default severity equals the constructed rule's meta severity, for every id.
        for id in RULE_IDS {
            let info = rule_info(&Config::default(), id).unwrap();
            let built = build_rule(id, &Value::Null, None).unwrap();
            assert_eq!(info.severity, built.meta().default_severity, "{id}");
            assert!(!info.severity_overridden, "{id}");
        }
    }
}
