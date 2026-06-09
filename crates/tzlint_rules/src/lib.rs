//! `tzlint_rules` — built-in native rules.
//!
//! Each rule implements the [`Rule`](tzlint_pdk::Rule) trait (from `tzlint_pdk`) and declares
//! the [`NodeKind`](tzlint_ast::NodeKind)s it visits, which the single-traversal scheduler in
//! `tzlint_core` dispatches. Per-node rules act in `check`; document-level rules (e.g.
//! [`NoMixedZenkakuHankakuAlphabet`] and [`JaNoMixedPeriod`]) register `ROOT` and walk the
//! subtree from `check`.
//!
//! [`RULE_IDS`] is the id list (single source of truth). [`build_rule`] constructs one rule by
//! id, applying config `options` (via the rule's `from_options`, where it has one) and an
//! optional severity override; [`builtin_rules`] is the default-constructed full set (every id,
//! default options, no override). The morphology-dependent [`NoDoubledJoshi`] declares
//! `with_morphology` and runs only when a Japanese morphology table is available.

pub mod rules;
mod util;

#[cfg(test)]
mod test_support;

use serde_json::Value;
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

use rules::{
    ja_no_mixed_period, ja_no_redundant_expression, ja_prh, max_kanji_continuous_len, max_ten,
    no_double_negative_ja, no_doubled_conjunctive_particle_ga, no_doubled_joshi,
    no_dropping_the_ra, no_exclamation_question_mark, no_hankaku_kana, no_mix_dearu_desumasu,
    no_mixed_zenkaku_hankaku_alphabet, no_nfd, no_todo, no_zero_width_spaces, sentence_length,
};
pub use rules::{
    ja_no_mixed_period::JaNoMixedPeriod, ja_no_redundant_expression::JaNoRedundantExpression,
    ja_prh::JaPrh, max_kanji_continuous_len::MaxKanjiContinuousLen, max_ten::MaxTen,
    no_double_negative_ja::NoDoubleNegativeJa,
    no_doubled_conjunctive_particle_ga::NoDoubledConjunctiveParticleGa,
    no_doubled_joshi::NoDoubledJoshi, no_dropping_the_ra::NoDroppingTheRa,
    no_exclamation_question_mark::NoExclamationQuestionMark, no_hankaku_kana::NoHankakuKana,
    no_mix_dearu_desumasu::NoMixDearuDesumasu,
    no_mixed_zenkaku_hankaku_alphabet::NoMixedZenkakuHankakuAlphabet, no_nfd::NoNfd,
    no_todo::NoTodo, no_zero_width_spaces::NoZeroWidthSpaces, sentence_length::SentenceLength,
};

/// The ids of every built-in rule, in [`builtin_rules`] order. Single source of truth — preset
/// keys in `tzlint_core` must match these verbatim.
pub const RULE_IDS: &[&str] = &[
    sentence_length::ID,
    max_ten::ID,
    max_kanji_continuous_len::ID,
    no_hankaku_kana::ID,
    no_mix_dearu_desumasu::ID,
    no_mixed_zenkaku_hankaku_alphabet::ID,
    no_nfd::ID,
    no_zero_width_spaces::ID,
    no_exclamation_question_mark::ID,
    ja_no_mixed_period::ID,
    no_todo::ID,
    no_doubled_joshi::ID,
    no_doubled_conjunctive_particle_ga::ID,
    ja_no_redundant_expression::ID,
    no_dropping_the_ra::ID,
    no_double_negative_ja::ID,
    ja_prh::ID,
];

/// Construct a single built-in rule by `id`, applying config `options` (through the rule's
/// `from_options`, where it has one — rules without options ignore it) and an optional
/// `severity` override. Returns `None` for an unknown id.
///
/// This is the one place a config rule entry becomes a rule instance; every id in [`RULE_IDS`]
/// must be handled here (a test enforces it).
pub fn build_rule(id: &str, options: &Value, severity: Option<Severity>) -> Option<Box<dyn Rule>> {
    let rule: Box<dyn Rule> = match id {
        sentence_length::ID => Box::new(SentenceLength::from_options(options)),
        max_ten::ID => Box::new(MaxTen::from_options(options)),
        max_kanji_continuous_len::ID => Box::new(MaxKanjiContinuousLen::from_options(options)),
        no_hankaku_kana::ID => Box::new(NoHankakuKana::new()),
        no_mix_dearu_desumasu::ID => Box::new(NoMixDearuDesumasu::from_options(options)),
        no_mixed_zenkaku_hankaku_alphabet::ID => Box::new(NoMixedZenkakuHankakuAlphabet::new()),
        no_nfd::ID => Box::new(NoNfd::new()),
        no_zero_width_spaces::ID => Box::new(NoZeroWidthSpaces::new()),
        no_exclamation_question_mark::ID => {
            Box::new(NoExclamationQuestionMark::from_options(options))
        }
        ja_no_mixed_period::ID => Box::new(JaNoMixedPeriod::new()),
        no_todo::ID => Box::new(NoTodo::from_options(options)),
        no_doubled_joshi::ID => Box::new(NoDoubledJoshi::from_options(options)),
        no_doubled_conjunctive_particle_ga::ID => Box::new(NoDoubledConjunctiveParticleGa::new()),
        ja_no_redundant_expression::ID => Box::new(JaNoRedundantExpression::new()),
        no_dropping_the_ra::ID => Box::new(NoDroppingTheRa::new()),
        no_double_negative_ja::ID => Box::new(NoDoubleNegativeJa::new()),
        ja_prh::ID => Box::new(JaPrh::from_options(options)),
        _ => return None,
    };
    Some(match severity {
        Some(severity) => Box::new(SeverityOverride::new(rule, severity)),
        None => rule,
    })
}

/// Every built-in rule, default-constructed (default options, no severity override) — in
/// [`RULE_IDS`] order. The registry the engine is wired through when no config narrows it.
pub fn builtin_rules() -> Vec<Box<dyn Rule>> {
    RULE_IDS
        .iter()
        .filter_map(|id| build_rule(id, &Value::Null, None))
        .collect()
}

/// Wraps a rule so it reports at a config-overridden severity.
///
/// The engine reads a rule's effective severity from [`RuleMeta::default_severity`] (it builds
/// the rule's [`Context`] from it) and never consults `&self` for severity, so a wrapper that
/// returns a cloned meta with the severity replaced — and otherwise delegates `check`/`finish`
/// to the inner rule — is sufficient. No engine or `Rule`-trait change is needed.
struct SeverityOverride {
    inner: Box<dyn Rule>,
    meta: RuleMeta,
}

impl SeverityOverride {
    fn new(inner: Box<dyn Rule>, severity: Severity) -> Self {
        let mut meta = inner.meta().clone();
        meta.default_severity = severity;
        SeverityOverride { inner, meta }
    }
}

impl Rule for SeverityOverride {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }
    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        self.inner.check(node, cx);
    }
    fn finish<'ast>(&self, cx: &mut Context<'ast>) {
        self.inner.finish(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_and_rule_ids_agree() {
        let rules = builtin_rules();
        assert_eq!(rules.len(), RULE_IDS.len());
        for (rule, id) in rules.iter().zip(RULE_IDS) {
            assert_eq!(rule.meta().id.as_str(), *id);
        }
    }

    #[test]
    fn builtin_morphology_rules_pin_a_language() {
        // Tripwire: a rule that needs morphology must pin a language, or
        // `RegionRules::required_langs` would silently drop it from the cache fingerprint. Vacuous
        // until a morphology built-in lands (M2l's `no-doubled-joshi`), then a real guard.
        for rule in builtin_rules() {
            let meta = rule.meta();
            assert_eq!(
                meta.needs_morphology(),
                meta.required_lang().is_some(),
                "rule {} violates the morphology ⇒ pinned-language invariant",
                meta.id.as_str(),
            );
        }
    }

    #[test]
    fn rule_language_applicability_is_tagged() {
        use tzlint_ast::morphology::Lang;

        // The R5 classification. JA-only rules run only on Japanese documents and never on
        // untagged (unset-language) text; every other rule is language-neutral and runs on any
        // document, including untagged text.
        const JA_ONLY: &[&str] = &[
            "sentence-length",
            "max-ten",
            "no-hankaku-kana",
            "ja-no-mixed-period",
            "no-doubled-joshi",                   // JA via its morphology pin
            "no-mix-dearu-desumasu",              // JA via its morphology pin
            "no-doubled-conjunctive-particle-ga", // JA via its morphology pin
            "ja-no-redundant-expression",         // JA via its morphology pin
            "no-dropping-the-ra",                 // JA via its morphology pin
            "no-double-negative-ja",              // JA via its morphology pin
            "ja-prh",                             // JA-only (surface terminology rule)
        ];

        for id in RULE_IDS {
            let meta_holder = build_rule(id, &Value::Null, None).expect("every id builds");
            let meta = meta_holder.meta();
            if JA_ONLY.contains(id) {
                assert!(meta.applies_to(Some(Lang::JA)), "{id} should apply to JA");
                assert!(
                    !meta.applies_to(Some(Lang::KO)),
                    "{id} (JA-only) should not apply to KO"
                );
                assert!(
                    !meta.applies_to(None),
                    "{id} (JA-only) should not fire on untagged text"
                );
            } else {
                assert!(
                    meta.applies_to(None),
                    "{id} (neutral) should run on untagged text"
                );
                assert!(
                    meta.applies_to(Some(Lang::JA)),
                    "{id} (neutral) should run on JA"
                );
                assert!(
                    meta.applies_to(Some(Lang::KO)),
                    "{id} (neutral) should run on KO"
                );
            }
        }
    }

    #[test]
    fn rules_construct_via_default() {
        // `Default` delegates to `new()`; exercise each so the delegation is covered.
        assert_eq!(
            SentenceLength::default().meta().id.as_str(),
            "sentence-length"
        );
        assert_eq!(MaxTen::default().meta().id.as_str(), "max-ten");
        assert_eq!(
            MaxKanjiContinuousLen::default().meta().id.as_str(),
            "max-kanji-continuous-len"
        );
        assert_eq!(
            NoHankakuKana::default().meta().id.as_str(),
            "no-hankaku-kana"
        );
        assert_eq!(
            NoMixedZenkakuHankakuAlphabet::default().meta().id.as_str(),
            "no-mixed-zenkaku-hankaku-alphabet"
        );
        assert_eq!(NoNfd::default().meta().id.as_str(), "no-nfd");
        assert_eq!(
            NoZeroWidthSpaces::default().meta().id.as_str(),
            "no-zero-width-spaces"
        );
        assert_eq!(
            NoExclamationQuestionMark::default().meta().id.as_str(),
            "no-exclamation-question-mark"
        );
        assert_eq!(
            JaNoMixedPeriod::default().meta().id.as_str(),
            "ja-no-mixed-period"
        );
        assert_eq!(NoTodo::default().meta().id.as_str(), "no-todo");
        assert_eq!(
            NoDoubledJoshi::default().meta().id.as_str(),
            "no-doubled-joshi"
        );
        assert_eq!(
            NoMixDearuDesumasu::default().meta().id.as_str(),
            "no-mix-dearu-desumasu"
        );
        assert_eq!(
            NoDoubledConjunctiveParticleGa::default().meta().id.as_str(),
            "no-doubled-conjunctive-particle-ga"
        );
        assert_eq!(
            JaNoRedundantExpression::default().meta().id.as_str(),
            "ja-no-redundant-expression"
        );
        assert_eq!(
            NoDroppingTheRa::default().meta().id.as_str(),
            "no-dropping-the-ra"
        );
        assert_eq!(
            NoDoubleNegativeJa::default().meta().id.as_str(),
            "no-double-negative-ja"
        );
    }

    #[test]
    fn presets_only_reference_built_in_rule_ids() {
        // tzlint_core's presets reference rule ids as plain strings (no dependency on this
        // crate). This cross-crate guard (tzlint_core is a dev-dependency here) catches a
        // preset that points at a renamed/typo'd or not-yet-implemented rule id.
        use std::collections::BTreeSet;

        use tzlint_core::{Config, Preset, resolve};

        let known: BTreeSet<&str> = RULE_IDS.iter().copied().collect();
        for preset in [Preset::JaBasic, Preset::JaTechnicalWriting] {
            let resolved = resolve(Some(preset), Config::default());
            for id in resolved.rules.keys() {
                assert!(
                    known.contains(id.as_str()),
                    "preset {} references unknown rule id {:?}",
                    preset.id(),
                    id.as_str()
                );
            }
        }
    }

    #[test]
    fn build_rule_covers_every_id_and_rejects_unknown() {
        for id in RULE_IDS {
            let rule = build_rule(id, &Value::Null, None)
                .unwrap_or_else(|| panic!("build_rule has no arm for {id}"));
            assert_eq!(rule.meta().id.as_str(), *id);
        }
        assert!(build_rule("definitely-not-a-rule", &Value::Null, None).is_none());
    }

    #[test]
    fn build_rule_applies_severity_override_preserving_kinds() {
        let error = build_rule("max-ten", &Value::Null, Some(Severity::Error)).unwrap();
        let hint = build_rule("max-ten", &Value::Null, Some(Severity::Hint)).unwrap();
        assert_eq!(error.meta().default_severity, Severity::Error);
        assert_eq!(hint.meta().default_severity, Severity::Hint);
        // The wrapper preserves the inner rule's id and node-kind scheduling.
        let plain = build_rule("max-ten", &Value::Null, None).unwrap();
        assert_eq!(error.meta().id, plain.meta().id);
        assert_eq!(error.meta().node_kinds, plain.meta().node_kinds);
    }

    #[test]
    fn build_rule_routes_options_to_the_rule() {
        use crate::test_support::diagnose;
        // `max-ten` flags a sentence whose `、` count exceeds `max`; routing `max` proves options
        // reach the constructed rule. One `、` is over `max:0` but under `max:9`.
        let src = "これは、テストです。\n";
        let strict = build_rule("max-ten", &serde_json::json!({ "max": 0 }), None).unwrap();
        let lenient = build_rule("max-ten", &serde_json::json!({ "max": 9 }), None).unwrap();
        assert!(
            !diagnose(strict.as_ref(), src).is_empty(),
            "max:0 should flag the comma"
        );
        assert!(
            diagnose(lenient.as_ref(), src).is_empty(),
            "max:9 should not flag a single comma"
        );
    }

    #[test]
    fn rule_ids_are_unique_and_kebab_case() {
        let mut seen = std::collections::BTreeSet::new();
        for id in RULE_IDS {
            assert!(seen.insert(*id), "duplicate rule id {id}");
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "rule id {id} is not kebab-case"
            );
        }
    }
}
