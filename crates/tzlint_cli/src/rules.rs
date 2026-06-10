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

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use tzlint_ast::morphology::Lang;
use tzlint_core::processor::{ColumnTarget, DelimitedConfig};
use tzlint_core::{Config, ProcessorConfig, RegionRules, RuleSetting};
use tzlint_pdk::{Rule, RuleId, Severity};
use tzlint_rules::{RULE_IDS, build_rule};

/// Build the boxed rule set for an explicit rule-settings map (every built-in on by default,
/// minus those turned off, each with its configured options/severity).
#[must_use]
pub fn resolve_rules_from_map(rules: &BTreeMap<RuleId, RuleSetting>) -> Vec<Box<dyn Rule>> {
    RULE_IDS
        .iter()
        .filter_map(|id| match rules.get(&RuleId::from(*id)) {
            Some(RuleSetting::Off) => None,
            Some(RuleSetting::On { severity, options }) => build_rule(id, options, *severity),
            None => build_rule(id, &Value::Null, None),
        })
        .collect()
}

/// Build the boxed rule set to run for `config`: every built-in rule (in [`RULE_IDS`] order)
/// except those a `config.rules` entry turns off, each constructed with its configured options
/// and severity.
#[must_use]
pub fn resolve_rules(config: &Config) -> Vec<Box<dyn Rule>> {
    resolve_rules_from_map(&config.rules)
}

/// Whether **any** rule runs anywhere under `config`: the base set, or any column overlay
/// (`formats.*.columns.*.rules`). A column overlay can re-enable a rule the base turned off, so a
/// check against the base alone (`resolve_rules`) is not enough to decide whether the run will
/// report nothing — the CLI uses this for that note.
#[must_use]
pub fn any_effective_rules(config: &Config) -> bool {
    if !resolve_rules(config).is_empty() {
        return true;
    }
    config.formats.values().any(|fmt| {
        fmt.columns.iter().any(|col| {
            // base ⊕ column overlay (column wins) — same layering as `region_rules_for`.
            let mut merged = config.rules.clone();
            for (id, setting) in &col.rules {
                merged.insert(id.clone(), setting.clone());
            }
            !resolve_rules_from_map(&merged).is_empty()
        })
    })
}

/// The [`ProcessorConfig`] for a file with extension `ext` under `config`: the matching
/// `formats.<ext>` section becomes the delimited extraction config, or empty for other formats.
#[must_use]
pub fn processor_config_for(config: &Config, ext: Option<&str>) -> ProcessorConfig {
    let Some(fmt) = ext.and_then(|e| config.formats.get(&e.to_ascii_lowercase())) else {
        return ProcessorConfig::default();
    };
    ProcessorConfig {
        delimited: Some(DelimitedConfig {
            // `delimiter` is guaranteed ASCII by config parsing (`ConfigError::NonAsciiDelimiter`),
            // so `c as u8` is lossless here; `0` means "use the processor default".
            delimiter: fmt.delimiter.map(|c| c as u8).unwrap_or(0),
            has_header: fmt.has_header,
            columns: fmt
                .columns
                .iter()
                .map(|c| ColumnTarget {
                    selector: c.selector.clone(),
                    parse_mode: c.parse_mode,
                })
                .collect(),
        }),
    }
}

/// Drop the rules that do not apply to the document language `lang` (R6 scoping).
///
/// A language-neutral rule always survives; a language-scoped rule survives only when `lang` is
/// one of its languages — and never when `lang` is `None` (an unset language runs only the neutral
/// rules). This is applied where the lint-time rule set is built (not in [`resolve_rules`] or
/// [`rule_infos`], so `rules list` still reports every configured rule regardless of language).
fn scope_to_language(rules: Vec<Box<dyn Rule>>, lang: Option<Lang>) -> Vec<Box<dyn Rule>> {
    rules
        .into_iter()
        .filter(|rule| rule.meta().applies_to(lang))
        .collect()
}

/// The [`RegionRules`] for a file with extension `ext` under `config`: a base set from
/// `config.rules`, plus one set per configured column (its overlay layered over the base). Both
/// are scoped to the document language ([`Config::document_lang`]): a JA-only rule does not run on
/// non-Japanese (or untagged) text.
#[must_use]
pub fn region_rules_for(config: &Config, ext: Option<&str>) -> RegionRules {
    let lang = config.document_lang();
    let mut rr = RegionRules::base_only(scope_to_language(resolve_rules(config), lang));
    if let Some(fmt) = ext.and_then(|e| config.formats.get(&e.to_ascii_lowercase())) {
        for col in &fmt.columns {
            // base ⊕ column overlay (column wins).
            let mut merged = config.rules.clone();
            for (id, setting) in &col.rules {
                merged.insert(id.clone(), setting.clone());
            }
            let rules = scope_to_language(resolve_rules_from_map(&merged), lang);
            match &col.selector {
                tzlint_core::processor::ColumnSelector::Index(one_based) => {
                    rr.push_column(one_based.checked_sub(1), None, rules);
                }
                tzlint_core::processor::ColumnSelector::Name(name) => {
                    rr.push_column(None, Some(name.clone()), rules);
                }
            }
        }
    }
    rr
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
            ..Default::default()
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

#[cfg(test)]
mod processing_builders {
    use super::*;
    use std::collections::BTreeMap;
    use tzlint_core::processor::{ColumnSelector, ParseMode};
    use tzlint_core::{ColumnConfig, Config, FormatConfig};

    fn csv_config() -> Config {
        let mut formats = BTreeMap::new();
        formats.insert(
            "csv".into(),
            FormatConfig {
                has_header: true,
                delimiter: None,
                columns: vec![ColumnConfig {
                    selector: ColumnSelector::Name("body".into()),
                    parse_mode: ParseMode::PlainText,
                    rules: BTreeMap::new(),
                }],
            },
        );
        Config {
            formats,
            ..Default::default()
        }
    }

    #[test]
    fn processor_config_for_csv_extension() {
        let config = csv_config();
        let pcfg = processor_config_for(&config, Some("csv"));
        let d = pcfg.delimited.unwrap();
        assert!(d.has_header);
        assert_eq!(d.columns.len(), 1);
        assert_eq!(d.columns[0].selector, ColumnSelector::Name("body".into()));
        assert_eq!(d.columns[0].parse_mode, ParseMode::PlainText);
        // A markdown file → no delimited config.
        assert!(
            processor_config_for(&config, Some("md"))
                .delimited
                .is_none()
        );
    }

    #[test]
    fn region_rules_for_scopes_rules_by_document_language() {
        use tzlint_core::RegionTag;
        let names = |config: &Config| -> Vec<String> {
            region_rules_for(config, None)
                .for_tag(&RegionTag::whole())
                .iter()
                .map(|r| r.meta().id.as_str().to_string())
                .collect()
        };

        // Language unset ⇒ only language-neutral rules run; JA-only rules are scoped out.
        let unset = names(&Config::default());
        assert!(
            unset.contains(&"no-nfd".to_string()),
            "a neutral rule still runs"
        );
        assert!(
            !unset.contains(&"sentence-length".to_string()),
            "a JA-only rule is scoped out when the language is unset"
        );
        assert!(
            !unset.contains(&"no-doubled-joshi".to_string()),
            "a JA morphology rule is scoped out when the language is unset"
        );

        // language: ja ⇒ JA rules run alongside the neutral ones.
        let ja = names(&Config {
            language: Some("ja".into()),
            ..Default::default()
        });
        assert!(
            ja.contains(&"sentence-length".to_string()),
            "JA-only runs under ja"
        );
        assert!(
            ja.contains(&"no-nfd".to_string()),
            "neutral still runs under ja"
        );
    }

    #[test]
    fn region_rules_for_layers_column_overlay_over_base() {
        use tzlint_core::RegionTag;
        // Base disables no-todo; the body column re-enables it. The whole region uses the base
        // (no-todo absent), while the `body` column region runs no-todo.
        let mut formats = BTreeMap::new();
        let mut overlay = BTreeMap::new();
        overlay.insert(
            RuleId::from("no-todo"),
            RuleSetting::On {
                severity: None,
                options: Value::Null,
            },
        );
        formats.insert(
            "csv".into(),
            FormatConfig {
                has_header: true,
                delimiter: None,
                columns: vec![ColumnConfig {
                    selector: ColumnSelector::Name("body".into()),
                    parse_mode: ParseMode::PlainText,
                    rules: overlay,
                }],
            },
        );
        let mut base_rules = BTreeMap::new();
        base_rules.insert(RuleId::from("no-todo"), RuleSetting::Off);
        let config = Config {
            rules: base_rules,
            formats,
            ..Default::default()
        };

        let rr = region_rules_for(&config, Some("csv"));
        let has = |rules: &[&dyn Rule], id: &str| rules.iter().any(|r| r.meta().id.as_str() == id);
        // The whole-region (base) set has no-todo OFF.
        assert!(!has(&rr.for_tag(&RegionTag::whole()), "no-todo"));
        // The `body` column set has no-todo back ON (column overlay wins).
        assert!(has(
            &rr.for_tag(&RegionTag::column(1, Some("body".into()))),
            "no-todo"
        ));
        // A markdown file (no csv overlay) yields only the base set, never the column overlay.
        let md = region_rules_for(&config, Some("md"));
        assert!(!has(&md.for_tag(&RegionTag::whole()), "no-todo"));
    }

    /// A config that disables every built-in rule in the base set.
    fn all_base_off() -> BTreeMap<RuleId, RuleSetting> {
        RULE_IDS
            .iter()
            .map(|id| (RuleId::from(*id), RuleSetting::Off))
            .collect()
    }

    #[test]
    fn any_effective_rules_false_when_base_off_and_no_overlay() {
        let config = Config {
            rules: all_base_off(),
            ..Default::default()
        };
        assert!(!any_effective_rules(&config));
    }

    #[test]
    fn any_effective_rules_true_when_a_column_overlay_re_enables_a_rule() {
        // Base disables everything, but the csv `body` column re-enables no-todo — so the run does
        // report something, and the "everything disabled" note must be suppressed.
        let mut overlay = BTreeMap::new();
        overlay.insert(
            RuleId::from("no-todo"),
            RuleSetting::On {
                severity: None,
                options: Value::Null,
            },
        );
        let mut formats = BTreeMap::new();
        formats.insert(
            "csv".into(),
            FormatConfig {
                has_header: true,
                delimiter: None,
                columns: vec![ColumnConfig {
                    selector: ColumnSelector::Name("body".into()),
                    parse_mode: ParseMode::PlainText,
                    rules: overlay,
                }],
            },
        );
        let config = Config {
            rules: all_base_off(),
            formats,
            ..Default::default()
        };
        assert!(any_effective_rules(&config));
    }
}
