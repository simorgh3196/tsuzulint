//! Resolving a [`Config`] to the set of [`Rule`]s the engine should run.
//!
//! Activation model: **every built-in rule is on by default**; a `config.rules` entry set to
//! `false` (→ [`RuleSetting::Off`]) disables that rule. So a bare `tzlint lint` runs the full
//! built-in set, and a config narrows it.
//!
//! Per-rule `options` and severity overrides are *not* applied yet: [`builtin_rules`] constructs
//! each rule with its defaults, and there is no construction-time seam to route config
//! `options`/`severity` through (the `Rule` trait exposes only an immutable
//! [`RuleMeta`](tzlint_pdk::RuleMeta)). That routing is a follow-up that adds a config-aware
//! constructor to `tzlint_rules`; until then a rule runs with its own defaults regardless of the
//! `options`/`severity` set in config. Unknown rule ids in config are surfaced via
//! [`unknown_rule_ids`] so a typo'd setting is not silently ignored.

use std::collections::BTreeSet;

use tzlint_core::{Config, RuleSetting};
use tzlint_pdk::Rule;
use tzlint_rules::{RULE_IDS, builtin_rules};

/// Build the boxed rule set to run for `config`: every built-in rule except those a
/// `config.rules` entry turns off.
#[must_use]
pub fn resolve_rules(config: &Config) -> Vec<Box<dyn Rule>> {
    builtin_rules()
        .into_iter()
        .filter(|rule| !matches!(config.rules.get(&rule.meta().id), Some(RuleSetting::Off)))
        .collect()
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
        // An explicit `On` (with options/severity that are not routed yet) still runs the rule.
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
}
