//! `no-dropping-the-ra` — flag ら抜き言葉: a 一段 or カ変 verb taking the potential auxiliary
//! 「れる」 where standard Japanese requires 「られる」 (e.g. 見れる → 見られる, 来れる → 来られる).
//!
//! A **morphology-dependent** rule (Japanese). The discriminator is precise: the auxiliary れる
//! attaches to a 一段/カ変 verb **only** in the ら抜き mistake — both the potential and the passive
//! of those verbs otherwise use られる. A 五段 verb + れる is the legitimate passive (書かれる) and
//! is therefore left alone, which is why the rule keys on the preceding verb's 活用型 (一段 / カ変).
//! base form `れる` vs `られる` on the auxiliary is the second half of the test. Keys on IPADIC
//! literals, so an unrecognized tagset matches nothing (a false negative, never a false positive).

use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-dropping-the-ra";

/// Flags a 一段/カ変 verb followed by the potential auxiliary れる (the ら抜き mistake).
pub struct NoDroppingTheRa {
    meta: RuleMeta,
}

impl NoDroppingTheRa {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoDroppingTheRa {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
        }
    }
}

impl Default for NoDroppingTheRa {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoDroppingTheRa {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let Some(table) = cx.morphology() else {
            return; // belt-and-suspenders over the engine's morphology gate
        };

        let mut tokens: Vec<&ArchivedToken> = cx.tokens_of(node.id()).collect();
        tokens.sort_by_key(|t| (t.surface().start, t.surface().end));

        // A 一段/カ変 verb immediately followed by the auxiliary れる (base form れる) is ら抜き.
        for pair in tokens.windows(2) {
            let [verb, aux] = pair else { continue };
            if verb.is_unknown() || aux.is_unknown() {
                continue;
            }
            if is_ichidan_or_kahen_verb(verb, table) && is_reru_auxiliary(aux, table) {
                cx.report(Span::new(verb.surface().start, aux.surface().end), MESSAGE);
            }
        }
    }
}

/// Whether `tok` is a verb whose 活用型 is 一段 or カ変 — the only conjugations for which a
/// following れる is ら抜き (五段 + れる is the legitimate passive).
fn is_ichidan_or_kahen_verb(tok: &ArchivedToken, table: &ArchivedMorphologyV1) -> bool {
    feature(tok, table, FeatureKey::POS) == Some("動詞")
        && feature(tok, table, FeatureKey::CONJUGATION_TYPE)
            .is_some_and(|c| c.starts_with("一段") || c.starts_with("カ変"))
}

/// Whether `tok` is the potential/passive auxiliary れる (base form れる, **not** られる).
fn is_reru_auxiliary(tok: &ArchivedToken, table: &ArchivedMorphologyV1) -> bool {
    // れる is tagged 動詞 (接尾) or 助動詞 depending on the dictionary; the base form is the signal.
    matches!(
        feature(tok, table, FeatureKey::POS),
        Some("動詞") | Some("助動詞")
    ) && tok.base_form(table) == Some("れる")
}

/// The Japanese diagnostic for a ら抜き expression.
const MESSAGE: &str = "ら抜き言葉がみつかりました。\n\n\
一段動詞・カ変動詞には「られる」を使ってください（例: 見れる → 見られる、来れる → 来られる）。";

/// Resolve a feature value (e.g. POS) of `token` against `table`, or `None`.
fn feature<'a>(
    token: &ArchivedToken,
    table: &'a ArchivedMorphologyV1,
    key: FeatureKey,
) -> Option<&'a str> {
    token
        .features(table)
        .find(|(k, _)| *k == key)
        .and_then(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{diagnose, diagnose_with_morphology};
    use tzlint_ast::morphology::{MorphologyBuilder, Tagset, TokenAttrs};
    use tzlint_ast::{NodeId, Span};

    fn push(
        b: &mut MorphologyBuilder,
        node: NodeId,
        start: u32,
        end: u32,
        base: Option<&str>,
        feats: &[(FeatureKey, &str)],
    ) {
        b.push_token(
            TokenAttrs {
                node,
                surface: Span::new(start, end),
                lang: Lang::JA,
                tagset: Tagset::IPADIC,
                flags: 0,
            },
            None,
            base,
            feats,
        );
    }

    /// A 動詞 with the given 活用型 (CONJUGATION_TYPE).
    fn verb(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32, conj: &str) {
        push(
            b,
            node,
            start,
            end,
            None,
            &[
                (FeatureKey::POS, "動詞"),
                (FeatureKey::CONJUGATION_TYPE, conj),
            ],
        );
    }
    /// The auxiliary れる/られる with `base` as its base form.
    fn aux(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32, base: &str) {
        push(
            b,
            node,
            start,
            end,
            Some(base),
            &[(FeatureKey::POS, "助動詞")],
        );
    }

    #[test]
    fn flags_ichidan_ra_nuki() {
        // 見れる — 見(一段) + れる(base れる) → ら抜き → flag 見..れる (0..9).
        let diags = diagnose_with_morphology(&NoDroppingTheRa::new(), "見れる", |p, b| {
            verb(b, p, 0, 3, "一段"); // 見
            aux(b, p, 3, 9, "れる"); // れる
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (0, 9));
        assert!(diags[0].message.contains("ら抜き"), "{}", diags[0].message);
    }

    #[test]
    fn the_correct_rareru_is_clean() {
        // 見られる — 見(一段) + られる(base られる) → correct → no flag.
        let diags = diagnose_with_morphology(&NoDroppingTheRa::new(), "見られる", |p, b| {
            verb(b, p, 0, 3, "一段"); // 見
            aux(b, p, 3, 12, "られる"); // られる
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn flags_kahen_ra_nuki() {
        // 来れる — 来(カ変・クル) + れる(base れる) → ら抜き.
        let diags = diagnose_with_morphology(&NoDroppingTheRa::new(), "来れる", |p, b| {
            verb(b, p, 0, 3, "カ変・クル"); // 来
            aux(b, p, 3, 9, "れる"); // れる
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (0, 9));
    }

    #[test]
    fn godan_passive_reru_is_clean() {
        // 書かれる — 書か(五段・カ行イ音便) + れる(base れる) → legitimate passive, NOT ら抜き.
        let diags = diagnose_with_morphology(&NoDroppingTheRa::new(), "書かれる", |p, b| {
            verb(b, p, 0, 6, "五段・カ行イ音便"); // 書か
            aux(b, p, 6, 12, "れる"); // れる
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        assert!(diagnose(&NoDroppingTheRa::new(), "見れる").is_empty());
    }
}
