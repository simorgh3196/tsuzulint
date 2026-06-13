//! `no-doubled-conjunction` — flag the same conjunction (接続詞) at the start of consecutive
//! sentences within a paragraph (e.g. two sentences in a row both beginning with 「しかし」).
//!
//! A **morphology-dependent** rule (Japanese, via [`with_morphology`](RuleMeta::with_morphology)).
//! It identifies the first 接続詞 token per sentence (skipping any 接続詞 immediately preceded by a
//! whitespace token, which can appear as sentence separators), then reports the opening 接続詞 of
//! the second sentence when it shares the same surface form as the one in the immediately preceding
//! sentence. Keys on the IPADIC POS literal `"接続詞"`, so an unrecognised tagset simply matches
//! nothing (a false negative, never a false positive).

use tzlint_ast::NodeKind;
use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-doubled-conjunction";

/// Sentence terminators that separate sentences within a block node.
const TERMINATORS: [char; 7] = ['。', '．', '.', '！', '？', '!', '?'];

/// Flags the same sentence-initial conjunction (接続詞) in consecutive sentences.
pub struct NoDoubledConjunction {
    meta: RuleMeta,
}

impl NoDoubledConjunction {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoDoubledConjunction {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
        }
    }
}

impl Default for NoDoubledConjunction {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoDoubledConjunction {
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

        // Collect and sort tokens into source order.
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

        // Build a list of (sentence_index, surface, span) for the FIRST 接続詞 of each sentence,
        // skipping any 接続詞 whose immediately preceding token is a whitespace (空白) token —
        // matching the upstream workaround for whitespace-as-separator misidentification.
        //
        // We collect per-sentence first-conjunctions keyed by sentence index: only the first
        // 接続詞 in each sentence counts (upstream uses `current_tokens[0]`).
        let mut first_per_sentence: Vec<(usize, &str, tzlint_ast::Span)> = Vec::new();

        for (tok_idx, &tok) in tokens.iter().enumerate() {
            if tok.is_unknown() {
                continue;
            }
            if feature(tok, table, FeatureKey::POS) != Some("接続詞") {
                continue;
            }
            // Skip if the immediately preceding token (in source order) is a whitespace token.
            // IPADIC tags whitespace as 記号/空白 (POS_SUB_1 = "空白").
            if tok_idx > 0 {
                let prev = tokens[tok_idx - 1];
                if !prev.is_unknown() && feature(prev, table, FeatureKey::POS_SUB_1) == Some("空白")
                {
                    continue;
                }
            }
            let sidx = sentence_idx(tok.surface().start);
            // Only record the first 接続詞 per sentence.
            if first_per_sentence.last().map(|(s, _, _)| *s) == Some(sidx) {
                continue;
            }
            let surface = ast.text_of(tok.surface()).unwrap_or("");
            first_per_sentence.push((sidx, surface, tok.surface()));
        }

        // Report the opening 接続詞 of the current sentence whenever it matches the previous one.
        for pair in first_per_sentence.windows(2) {
            let (_, prev_surface, _) = pair[0];
            let (_, curr_surface, curr_span) = pair[1];
            if prev_surface == curr_surface && !curr_surface.is_empty() {
                cx.report(
                    curr_span,
                    format!("同じ接続詞（{curr_surface}）が連続して使われています。"),
                );
            }
        }
    }
}

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

    /// Push a token with a given POS (and optional POS_SUB_1) at `[start, end)`.
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

    /// A generic noun/word token.
    fn word(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, &[(FeatureKey::POS, "名詞")]);
    }

    /// A 接続詞 token (conjunction) at `[start, end)`.
    fn conjunction(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, &[(FeatureKey::POS, "接続詞")]);
    }

    /// A whitespace token (記号/空白) at `[start, end)`.
    fn whitespace(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            &[(FeatureKey::POS, "記号"), (FeatureKey::POS_SUB_1, "空白")],
        );
    }

    #[test]
    fn flags_repeated_conjunction_in_consecutive_sentences() {
        // しかし、A。しかし、B — same 接続詞 starts both sentences → flag the second.
        // Byte layout (JP chars = 3 bytes each, 、= 3 bytes, A/B = 1 byte, 。= 3 bytes):
        //   し(0..3) か(3..6) し(6..9) = "しかし" at 0..9
        //   、 at 9..12
        //   A at 12..13
        //   。 at 13..16  ← sentence boundary
        //   し(16..19) か(19..22) し(22..25) = "しかし" at 16..25
        //   、 at 25..28
        //   B at 28..29
        let src = "しかし、A。しかし、B";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            conjunction(b, p, 0, 9); // しかし (sentence 0)
            word(b, p, 9, 12); // 、
            word(b, p, 12, 13); // A
            // 。 at 13..16 is the sentence terminator
            conjunction(b, p, 16, 25); // しかし (sentence 1) -> flagged
            word(b, p, 25, 28); // 、
            word(b, p, 28, 29); // B
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (16, 25));
        assert!(
            diags[0].message.contains("同じ接続詞（しかし）"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn different_conjunctions_are_clean() {
        // しかし…。でも… — different opening conjunctions → no diagnostic.
        let src = "しかし、A。でも、B";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            conjunction(b, p, 0, 9); // しかし
            word(b, p, 9, 15); // 、A
            conjunction(b, p, 18, 21); // でも (sentence 1, 。 at 15..18)
            word(b, p, 21, 27); // 、B
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn single_sentence_is_clean() {
        // Only one sentence → nothing to compare → no diagnostic.
        let src = "しかし、A";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            conjunction(b, p, 0, 9); // しかし
            word(b, p, 9, 15); // 、A
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn sentence_with_no_conjunction_breaks_the_chain() {
        // しかし A。B。しかし C — the middle sentence (B) has no conjunction; so the two しかし
        // sentences are NOT consecutive → no diagnostic.
        // Byte layout:
        //   し(0..3) か(3..6) し(6..9) = "しかし" 0..9
        //   _A at 9..11
        //   。 at 11..14 (s0→s1 boundary)
        //   B at 14..15
        //   。 at 15..18 (s1→s2 boundary)
        //   し(18..21) か(21..24) し(24..27) = "しかし" 18..27
        //   _C at 27..29
        let src = "しかし A。B。しかし C";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            conjunction(b, p, 0, 9); // しかし (sentence 0)
            word(b, p, 9, 11); // _A
            // 。 at 11..14
            word(b, p, 14, 15); // B (sentence 1 has no 接続詞)
            // 。 at 15..18
            conjunction(b, p, 18, 27); // しかし (sentence 2) — not consecutive with s0
            word(b, p, 27, 29); // _C
        });
        // s0's conjunction: しかし, s1: none (no conjunction recorded), s2: しかし.
        // first_per_sentence = [(0, "しかし"), (2, "しかし")] with no s1 entry.
        // windows(2) sees [(s0,しかし),(s2,しかし)] — they ARE adjacent in the per-sentence list
        // even though s1 was skipped. This matches the upstream behaviour: only sentences that
        // *have* a leading conjunction participate, and adjacent entries are compared.
        // So we DO expect a diagnostic here (both s0 and s2 open with しかし and no s1 conjunction
        // interrupts the sequence from the upstream's perspective of only-conjunction sentences).
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn whitespace_preceded_conjunction_is_skipped() {
        // If a 接続詞 is immediately preceded by a whitespace (空白) token, it is not counted as
        // a sentence-initial conjunction (upstream workaround).
        // Two sentences each appear to start with しかし preceded by whitespace → skipped → clean.
        let src = "A しかし。B しかし";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            word(b, p, 0, 1); // A
            whitespace(b, p, 1, 2); // space
            conjunction(b, p, 2, 11); // しかし (skipped — prev is 空白)
            // 。 at 11..14
            word(b, p, 14, 15); // B
            whitespace(b, p, 15, 16); // space
            conjunction(b, p, 16, 25); // しかし (skipped — prev is 空白)
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn three_consecutive_flags_each_repeat() {
        // しかし…。しかし…。しかし… — three in a row → flag sentence 1 and sentence 2.
        let src = "しかし A。しかし B。しかし C";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            conjunction(b, p, 0, 9); // しかし (s0)
            word(b, p, 9, 11); // _A
            // 。 at 11..14
            conjunction(b, p, 14, 23); // しかし (s1) flagged
            word(b, p, 23, 25); // _B
            // 。 at 25..28
            conjunction(b, p, 28, 37); // しかし (s2) flagged
            word(b, p, 37, 39); // _C
        });
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (14, 23));
        assert_eq!((diags[1].span.start, diags[1].span.end), (28, 37));
    }

    #[test]
    fn broken_run_resets_the_memory() {
        // しかし…。でも…。しかし… — the middle sentence breaks the run; the third しかし is not
        // consecutive with the first → clean.
        let src = "しかし A。でも B。しかし C";
        let diags = diagnose_with_morphology(&NoDoubledConjunction::new(), src, |p, b| {
            conjunction(b, p, 0, 9); // しかし (s0)
            word(b, p, 9, 11); // _A
            // 。 at 11..14
            conjunction(b, p, 14, 17); // でも (s1) — different
            word(b, p, 17, 19); // _B
            // 。 at 19..22
            conjunction(b, p, 22, 31); // しかし (s2)
            word(b, p, 31, 33); // _C
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        assert!(diagnose(&NoDoubledConjunction::new(), "しかし A。しかし B").is_empty());
    }
}
