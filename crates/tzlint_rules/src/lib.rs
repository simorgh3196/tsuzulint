//! `tzlint_rules` — built-in native rules.
//!
//! Each rule implements the [`Rule`](tzlint_pdk::Rule) trait (from `tzlint_pdk`) and declares
//! the [`NodeKind`](tzlint_ast::NodeKind)s it visits, which the single-traversal scheduler in
//! `tzlint_core` dispatches. Per-node rules act in `check`; the one document-level rule
//! ([`NoMixedZenkakuHankakuAlphabet`]) registers `ROOT` and walks the subtree from `check`.
//!
//! [`builtin_rules`] returns every shipped rule (the registry the engine is wired through);
//! [`RULE_IDS`] is the matching id list. Rule options are parsed leniently per rule
//! (`from_options`), but the engine does not yet route config options into rule instances, so
//! [`builtin_rules`] constructs each rule with its defaults — see the per-rule `from_options`
//! and the `TODO` below. Morphology-dependent rules (e.g. `no-doubled-joshi`) are deferred to
//! M2 and are not in this crate yet.

pub mod rules;
mod util;

#[cfg(test)]
mod test_support;

use tzlint_pdk::Rule;

use rules::{
    max_kanji_continuous_len, max_ten, no_hankaku_kana, no_mixed_zenkaku_hankaku_alphabet,
    sentence_length,
};
pub use rules::{
    max_kanji_continuous_len::MaxKanjiContinuousLen, max_ten::MaxTen,
    no_hankaku_kana::NoHankakuKana,
    no_mixed_zenkaku_hankaku_alphabet::NoMixedZenkakuHankakuAlphabet,
    sentence_length::SentenceLength,
};

/// The ids of every built-in rule, in [`builtin_rules`] order. Single source of truth — preset
/// keys in `tzlint_core` must match these verbatim.
pub const RULE_IDS: &[&str] = &[
    sentence_length::ID,
    max_ten::ID,
    max_kanji_continuous_len::ID,
    no_hankaku_kana::ID,
    no_mixed_zenkaku_hankaku_alphabet::ID,
];

/// Every built-in rule, default-constructed.
///
/// TODO: once `tzlint_core` routes resolved config options into rule construction, build these
/// via each rule's `from_options` instead of `new`.
pub fn builtin_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(SentenceLength::new()),
        Box::new(MaxTen::new()),
        Box::new(MaxKanjiContinuousLen::new()),
        Box::new(NoHankakuKana::new()),
        Box::new(NoMixedZenkakuHankakuAlphabet::new()),
    ]
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
