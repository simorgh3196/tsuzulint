//! `no-double-negative-ja` — flag a rhetorical double negative (二重否定): a negative earlier in a
//! sentence closed by 「は」+ another negative (e.g. ないことはない / なくはない / ないわけではない),
//! which reads more directly stated in the affirmative.
//!
//! A **morphology-dependent** rule (Japanese). To stay high-precision it matches one well-defined
//! shape: within a sentence, the run 「は」 (助詞) → negative, with at least one **earlier** negative
//! in the same sentence. A "negative" is the 助動詞 ない/ぬ/ず or the 補助形容詞「ない」. This
//! deliberately does NOT flag a single negation (問題ではない), nor the grammatical 〜なければならない
//! (no 「は」 before the closing negative) — both false positives a looser scan would produce. The
//! pattern set is narrow and extensible; it keys on IPADIC literals, so an unrecognized tagset
//! matches nothing (a false negative, never a false positive).

use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_ast::{ArchivedAst, NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-double-negative-ja";

/// Sentence terminators that bound the per-sentence scan.
const TERMINATORS: [char; 7] = ['。', '．', '.', '！', '？', '!', '?'];

/// Flags a 「…ない…はない」-shape rhetorical double negative.
pub struct NoDoubleNegativeJa {
    meta: RuleMeta,
}

impl NoDoubleNegativeJa {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoDoubleNegativeJa {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
        }
    }
}

impl Default for NoDoubleNegativeJa {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoDoubleNegativeJa {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let Some(table) = cx.morphology() else {
            return; // belt-and-suspenders over the engine's morphology gate
        };
        let ast = cx.ast();
        let base = node.span().start;
        let text = node.text();

        let mut tokens: Vec<&ArchivedToken> = cx.tokens_of(node.id()).collect();
        tokens.sort_by_key(|t| (t.surface().start, t.surface().end));

        let sentence_idx = |start: u32| -> usize {
            let upto = start.saturating_sub(base) as usize;
            text.get(..upto)
                .unwrap_or("")
                .chars()
                .filter(|c| TERMINATORS.contains(c))
                .count()
        };

        // Within each sentence, the closing 「は」+ negative is a double negative when an earlier
        // negative is present. Track the running count of negatives seen in the current sentence.
        let mut current_sentence: Option<usize> = None;
        let mut negatives_before = 0u32;
        for (i, &tok) in tokens.iter().enumerate() {
            let idx = sentence_idx(tok.surface().start);
            if current_sentence != Some(idx) {
                current_sentence = Some(idx);
                negatives_before = 0;
            }
            // A 「は」 immediately followed by a negative, with ≥1 earlier negative this sentence.
            if negatives_before >= 1
                && is_wa(tok, table, ast)
                && tokens
                    .get(i + 1)
                    .is_some_and(|next| is_negative(next, table))
            {
                let closing = tokens[i + 1];
                cx.report(
                    Span::new(tok.surface().start, closing.surface().end),
                    MESSAGE,
                );
            }
            if is_negative(tok, table) {
                negatives_before += 1;
            }
        }
    }
}

/// Whether `tok` is a negation: the 助動詞 ない/ぬ/ず or the 補助形容詞「ない」.
fn is_negative(tok: &ArchivedToken, table: &ArchivedMorphologyV1) -> bool {
    if tok.is_unknown() {
        return false;
    }
    match feature(tok, table, FeatureKey::POS) {
        Some("助動詞") => matches!(tok.base_form(table), Some("ない") | Some("ぬ") | Some("ず")),
        Some("形容詞") => tok.base_form(table) == Some("ない"),
        _ => false,
    }
}

/// Whether `tok` is the binding particle「は」(助詞, surface は).
fn is_wa(tok: &ArchivedToken, table: &ArchivedMorphologyV1, ast: &ArchivedAst) -> bool {
    !tok.is_unknown()
        && feature(tok, table, FeatureKey::POS) == Some("助詞")
        && ast.text_of(tok.surface()) == Some("は")
}

/// The Japanese diagnostic for a double negative.
const MESSAGE: &str = "二重否定がみつかりました。\n\n\
否定の否定はまわりくどく読みにくいので、肯定の表現に言い換えられないか検討してください\
（例: できないことはない → できる）。";

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

    fn word(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, None, &[(FeatureKey::POS, "名詞")]);
    }
    /// The 助動詞 negative ない (base ない).
    fn nai_aux(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            Some("ない"),
            &[(FeatureKey::POS, "助動詞")],
        );
    }
    /// The 補助形容詞 ない (base ない).
    fn nai_adj(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            Some("ない"),
            &[(FeatureKey::POS, "形容詞")],
        );
    }
    /// The binding particle は.
    fn wa(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            None,
            &[(FeatureKey::POS, "助詞"), (FeatureKey::POS_SUB_1, "係助詞")],
        );
    }

    #[test]
    fn flags_nai_koto_wa_nai() {
        // ないことはない — ない(0..6) こと(6..12) は(12..15) ない(15..21) → flag は..ない (12..21).
        let diags = diagnose_with_morphology(
            &NoDoubleNegativeJa::new(),
            "ないことはない",
            |p, b| {
                nai_aux(b, p, 0, 6); // ない (first negative)
                word(b, p, 6, 12); // こと
                wa(b, p, 12, 15); // は
                nai_adj(b, p, 15, 21); // ない (closing negative)
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (12, 21));
        assert!(
            diags[0].message.contains("二重否定"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_naku_wa_nai() {
        // なくはない — なく(形容詞 ない, 0..6) は(6..9) ない(形容詞 ない, 9..15) → flagged.
        let diags =
            diagnose_with_morphology(&NoDoubleNegativeJa::new(), "なくはない", |p, b| {
                nai_adj(b, p, 0, 6); // なく (補助形容詞 ない, first negative)
                wa(b, p, 6, 9); // は
                nai_adj(b, p, 9, 15); // ない (closing)
            });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (6, 15));
    }

    #[test]
    fn a_single_negation_with_wa_is_clean() {
        // 問題はない — は+ない but NO earlier negative (問題 is a noun) → not a double negative.
        let diags =
            diagnose_with_morphology(&NoDoubleNegativeJa::new(), "問題はない", |p, b| {
                word(b, p, 0, 6); // 問題
                wa(b, p, 6, 9); // は
                nai_aux(b, p, 9, 15); // ない
            });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_lone_negation_is_clean() {
        // 行かない — one negative, no は+negative closing → clean.
        let diags = diagnose_with_morphology(&NoDoubleNegativeJa::new(), "行かない", |p, b| {
            push(b, p, 0, 6, Some("行く"), &[(FeatureKey::POS, "動詞")]); // 行か
            nai_aux(b, p, 6, 12); // ない
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_sentence_boundary_separates_two_negatives() {
        // 行かない。問題はない — the earlier negative is in sentence 0; the は+ない is in sentence 1
        // with no earlier negative there → clean (the count resets per sentence).
        let diags = diagnose_with_morphology(
            &NoDoubleNegativeJa::new(),
            "行かない。問題はない",
            |p, b| {
                push(b, p, 0, 6, Some("行く"), &[(FeatureKey::POS, "動詞")]); // 行か
                nai_aux(b, p, 6, 12); // ない  (。 12..15)
                word(b, p, 15, 21); // 問題
                wa(b, p, 21, 24); // は
                nai_aux(b, p, 24, 30); // ない
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        assert!(diagnose(&NoDoubleNegativeJa::new(), "ないことはない").is_empty());
    }
}
