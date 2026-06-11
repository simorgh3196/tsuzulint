//! `no-doubled-conjunctive-particle-ga` — flag the 逆接の接続助詞「が」 used more than once in a
//! single sentence (e.g. 「Aだが、Bするが、C」), which muddles what connects to what.
//!
//! A **morphology-dependent** rule (Japanese, via [`with_morphology`](RuleMeta::with_morphology)).
//! It counts only the **conjunctive**「が」— a 助詞 whose surface is `が` and whose IPADIC POS
//! sub-type is `接続助詞` — so the subject-marking 格助詞「が」 is never counted. When a sentence
//! holds two or more, every occurrence after the first is reported. Keys on the IPADIC POS
//! literals, so an unrecognized tagset simply matches nothing (a false negative, never a false
//! positive — mirrors `no-doubled-joshi`).

use tzlint_ast::NodeKind;
use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-doubled-conjunctive-particle-ga";

/// Sentence terminators that reset the per-sentence count.
const TERMINATORS: [char; 7] = ['。', '．', '.', '！', '？', '!', '?'];

/// Flags the conjunctive「が」repeated within one sentence.
pub struct NoDoubledConjunctiveParticleGa {
    meta: RuleMeta,
}

impl NoDoubledConjunctiveParticleGa {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoDoubledConjunctiveParticleGa {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
        }
    }
}

impl Default for NoDoubledConjunctiveParticleGa {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoDoubledConjunctiveParticleGa {
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

        // The sentence index of a byte offset = the number of terminators before it in the block.
        let sentence_idx = |start: u32| -> usize {
            let upto = start.saturating_sub(base) as usize;
            text.get(..upto)
                .unwrap_or("")
                .chars()
                .filter(|c| TERMINATORS.contains(c))
                .count()
        };

        // Walk source-ordered tokens; per sentence, the first conjunctive が is allowed and every
        // later one is reported.
        let mut current_sentence: Option<usize> = None;
        let mut seen_in_sentence = 0u32;
        for &tok in &tokens {
            if !is_conjunctive_ga(tok, table, ast) {
                continue;
            }
            let idx = sentence_idx(tok.surface().start);
            if current_sentence != Some(idx) {
                current_sentence = Some(idx);
                seen_in_sentence = 0;
            }
            seen_in_sentence += 1;
            if seen_in_sentence >= 2 {
                cx.report(tok.surface(), MESSAGE);
            }
        }
    }
}

/// Whether `tok` is a trusted conjunctive「が」 (助詞 / 接続助詞 / surface `が`).
fn is_conjunctive_ga(
    tok: &ArchivedToken,
    table: &ArchivedMorphologyV1,
    ast: &tzlint_ast::ArchivedAst,
) -> bool {
    if tok.is_unknown() {
        return false;
    }
    feature(tok, table, FeatureKey::POS) == Some("助詞")
        && feature(tok, table, FeatureKey::POS_SUB_1) == Some("接続助詞")
        && ast.text_of(tok.surface()) == Some("が")
}

/// The Japanese diagnostic for a repeated conjunctive が.
const MESSAGE: &str = "一文に二回以上利用されている接続助詞「が」がみつかりました。\n\n\
逆接の「が」を一文に複数置くと係り受けが分かりにくくなります。文を分割するか、\
二つ目以降の「が」を別の表現に書き換えてください。";

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

    /// Push a token with major POS `pos` and optional sub-type at `[start, end)`.
    fn push(
        b: &mut MorphologyBuilder,
        node: NodeId,
        start: u32,
        end: u32,
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
            None,
            feats,
        );
    }

    fn word(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, &[(FeatureKey::POS, "名詞")]);
    }
    /// A conjunctive が at `[start, end)` (surface is whatever the source has there).
    fn conj_ga(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            &[
                (FeatureKey::POS, "助詞"),
                (FeatureKey::POS_SUB_1, "接続助詞"),
            ],
        );
    }
    /// A subject-marking 格助詞 が at `[start, end)`.
    fn case_ga(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            &[(FeatureKey::POS, "助詞"), (FeatureKey::POS_SUB_1, "格助詞")],
        );
    }

    #[test]
    fn flags_the_second_conjunctive_ga_in_a_sentence() {
        // 雨だが寒いが行く — two 接続助詞 が in one sentence → flag the second (at 18..21).
        let diags = diagnose_with_morphology(
            &NoDoubledConjunctiveParticleGa::new(),
            "雨だが寒いが行く",
            |p, b| {
                word(b, p, 0, 6); // 雨だ
                conj_ga(b, p, 6, 9); // が (1st)
                word(b, p, 9, 15); // 寒い
                conj_ga(b, p, 15, 18); // が (2nd) -> flagged
                word(b, p, 18, 24); // 行く
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (15, 18));
        assert!(
            diags[0].message.contains("接続助詞「が」"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn a_single_conjunctive_ga_is_clean() {
        let diags = diagnose_with_morphology(
            &NoDoubledConjunctiveParticleGa::new(),
            "雨だが行く",
            |p, b| {
                word(b, p, 0, 6); // 雨だ
                conj_ga(b, p, 6, 9); // が
                word(b, p, 9, 15); // 行く
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn the_case_marking_ga_is_not_counted() {
        // 雨が降るが寒い — a 格助詞 が (subject) then one 接続助詞 が → only one conjunctive → clean.
        let diags = diagnose_with_morphology(
            &NoDoubledConjunctiveParticleGa::new(),
            "雨が降るが寒い",
            |p, b| {
                word(b, p, 0, 3); // 雨
                case_ga(b, p, 3, 6); // が (格助詞 — not counted)
                word(b, p, 6, 12); // 降る
                conj_ga(b, p, 12, 15); // が (接続助詞 — first conjunctive)
                word(b, p, 15, 21); // 寒い
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_sentence_boundary_resets_the_count() {
        // 雨だが。寒いが。 — one 接続助詞 が per sentence → never doubled.
        let diags = diagnose_with_morphology(
            &NoDoubledConjunctiveParticleGa::new(),
            "雨だが。寒いが。",
            |p, b| {
                word(b, p, 0, 6); // 雨だ
                conj_ga(b, p, 6, 9); // が   (。 9..12)
                word(b, p, 12, 18); // 寒い
                conj_ga(b, p, 18, 21); // が  (。 21..24)
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn three_ga_flags_the_second_and_third() {
        // Aが Bが Cが (all 接続助詞, one sentence) → the 2nd and 3rd are reported.
        let diags = diagnose_with_morphology(
            &NoDoubledConjunctiveParticleGa::new(),
            "甲が乙が丙が",
            |p, b| {
                word(b, p, 0, 3); // 甲
                conj_ga(b, p, 3, 6); // が (1st)
                word(b, p, 6, 9); // 乙
                conj_ga(b, p, 9, 12); // が (2nd) -> flagged
                word(b, p, 12, 15); // 丙
                conj_ga(b, p, 15, 18); // が (3rd) -> flagged
            },
        );
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (9, 12));
        assert_eq!((diags[1].span.start, diags[1].span.end), (15, 18));
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        assert!(diagnose(&NoDoubledConjunctiveParticleGa::new(), "雨だが寒いが行く").is_empty());
    }
}
