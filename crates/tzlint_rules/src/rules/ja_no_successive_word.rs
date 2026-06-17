//! `ja-no-successive-word` — flag the same word appearing twice in immediate succession
//! (e.g. 「私は 私は」, 「の の」), which is typically a typo or copy-paste error.
//!
//! A **morphology-dependent** rule (Japanese, via [`with_morphology`](RuleMeta::with_morphology)).
//! Adjacent tokens are compared by surface form; the second of the duplicate pair is reported.
//! Two categories are excluded by default:
//!
//! - **漢数字** (`名詞` + `POS_SUB_1 = "数"`): repeated kanji numerals are idiomatic
//!   (e.g. 「九九」 = multiplication table).
//! - **オノマトペ** (katakana-only surface): repeated onomatopoeia is idiomatic
//!   (e.g. 「ドキドキ」, `allowOnomatopee` is `true` by default).
//!
//! Both exclusions can be overridden via `from_options`. An `allow` list (surface strings,
//! RegExp-style matching is not supported here — exact match only) further suppresses specific
//! words. Keys on all POS values, so an unrecognised tagset still compares surfaces — the rule
//! fires on any token sequence regardless of tagset; `is_unknown()` tokens are excluded.

use serde_json::Value;
use tzlint_ast::NodeKind;
use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-no-successive-word";

/// Flags the same word used twice in immediate succession.
pub struct JaNoSuccessiveWord {
    meta: RuleMeta,
    /// Whether to exempt katakana-only surfaces (オノマトペ, e.g. 「ドキドキ」). Default: `true`.
    allow_onomatopee: bool,
    /// Additional surfaces to suppress (exact match).
    allow: Vec<String>,
}

impl JaNoSuccessiveWord {
    /// Construct with default options (`allow_onomatopee` = true, empty `allow`).
    pub fn new() -> Self {
        JaNoSuccessiveWord {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
            allow_onomatopee: true,
            allow: Vec::new(),
        }
    }

    /// Construct from config `options`, leniently (missing/wrong-typed values keep defaults):
    /// `allowOnomatopee` (bool), `allow` (array of surface strings).
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(v) = options.get("allowOnomatopee").and_then(Value::as_bool) {
            rule.allow_onomatopee = v;
        }
        if let Some(array) = options.get("allow").and_then(Value::as_array) {
            rule.allow = array
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        rule
    }
}

impl Default for JaNoSuccessiveWord {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaNoSuccessiveWord {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let Some(table) = cx.morphology() else {
            return; // belt-and-suspenders over the engine's morphology gate
        };
        let ast = cx.ast();

        // Collect and sort tokens into source order.
        let mut tokens: Vec<&ArchivedToken> = cx.tokens_of(node.id()).collect();
        tokens.sort_by_key(|t| (t.surface().start, t.surface().end));

        // Walk adjacent pairs: compare `prev` and `curr` by surface form.
        let mut prev_surface: Option<&str> = None;
        let mut prev_is_number = false;
        for &tok in &tokens {
            if tok.is_unknown() {
                prev_surface = None;
                prev_is_number = false;
                continue;
            }
            let surface = match ast.text_of(tok.surface()) {
                Some(s) => s,
                None => {
                    prev_surface = None;
                    prev_is_number = false;
                    continue;
                }
            };
            let curr_is_number = is_kanji_number(tok, table);

            if let Some(prev) = prev_surface
                && prev == surface
            {
                // Exception 1: both tokens are 漢数字 (e.g. 「九九」) → idiomatic.
                if prev_is_number && curr_is_number {
                    prev_surface = Some(surface);
                    prev_is_number = curr_is_number;
                    continue;
                }
                // Exception 2: katakana-only surface → onomatopoeia → idiomatic (if enabled).
                if self.allow_onomatopee && is_katakana_only(surface) {
                    prev_surface = Some(surface);
                    prev_is_number = curr_is_number;
                    continue;
                }
                // Exception 3: explicit allow list.
                if self.allow.iter().any(|a| a == surface) {
                    prev_surface = Some(surface);
                    prev_is_number = curr_is_number;
                    continue;
                }
                cx.report(
                    tok.surface(),
                    format!("\"{}\" が連続して2回使われています。", surface),
                );
            }

            prev_surface = Some(surface);
            prev_is_number = curr_is_number;
        }
    }
}

/// Whether `tok` is a 漢数字 token (名詞 + POS_SUB_1 = 数).
fn is_kanji_number(tok: &ArchivedToken, table: &ArchivedMorphologyV1) -> bool {
    feature(tok, table, FeatureKey::POS) == Some("名詞")
        && feature(tok, table, FeatureKey::POS_SUB_1) == Some("数")
}

/// Whether `s` consists entirely of katakana characters and the long-vowel mark (ー).
/// Matches the upstream `isOnomatopee` regex: `/^[ァ-ロワヲンー]*$/`.
fn is_katakana_only(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars().all(|c| {
        // ァ (U+30A1) .. ロ (U+30ED), plus ワ (U+30EF), ヲ (U+30F2), ン (U+30F3), ー (U+30FC).
        // The upstream regex class [ァ-ロワヲンー] is unusual — it does not include ヰ/ヱ/ヴ etc.
        // We replicate it exactly.
        matches!(
            c,
            '\u{30A1}'..='\u{30ED}' | '\u{30EF}' | '\u{30F2}' | '\u{30F3}' | '\u{30FC}'
        )
    })
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

    /// Push a token with given features at `[start, end)`.
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

    /// A plain noun token.
    fn noun(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, &[(FeatureKey::POS, "名詞")]);
    }

    /// A 漢数字 token (名詞 + 数).
    fn kanji_num(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            &[(FeatureKey::POS, "名詞"), (FeatureKey::POS_SUB_1, "数")],
        );
    }

    /// A particle token (助詞).
    fn particle(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, &[(FeatureKey::POS, "助詞")]);
    }

    #[test]
    fn flags_adjacent_duplicate_noun() {
        // 「私は 私は」 — two 私 tokens adjacent (treating は as unrelated).
        // We'll use two adjacent 私 noun tokens for simplicity.
        // 私(0..3) 私(3..6)
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "私私", |p, b| {
            noun(b, p, 0, 3);
            noun(b, p, 3, 6);
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (3, 6));
        assert!(
            diags[0].message.contains("\"私\" が連続して2回"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn different_adjacent_words_are_clean() {
        // 「私は彼は」 — different surfaces → no diagnostic.
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "私は彼は", |p, b| {
            noun(b, p, 0, 3); // 私
            particle(b, p, 3, 6); // は
            noun(b, p, 6, 9); // 彼
            particle(b, p, 9, 12); // は
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn kanji_number_duplicate_is_exempt() {
        // 「九九」 — two 九 kanji-number tokens → idiomatic, not flagged.
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "九九", |p, b| {
            kanji_num(b, p, 0, 3); // 九 (1st)
            kanji_num(b, p, 3, 6); // 九 (2nd)
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn kanji_number_exempt_only_when_both_are_numbers() {
        // If one is a 名詞/数 and the other is a plain 名詞, the exemption does NOT apply.
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "九九", |p, b| {
            kanji_num(b, p, 0, 3); // 九 (数)
            noun(b, p, 3, 6); // 九 (plain noun — not a number)
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn katakana_duplicate_is_exempt_by_default() {
        // 「ドキドキ」-style: two identical katakana tokens → onomatopoeia exempt.
        // ドキ is within the katakana range ァ-ロ.
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "ドキドキ", |p, b| {
            noun(b, p, 0, 6); // ドキ (3 bytes each katakana char × 2 chars)
            noun(b, p, 6, 12); // ドキ
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn katakana_duplicate_flagged_when_allow_onomatopee_false() {
        // With `allowOnomatopee:false`, even katakana repetitions are flagged.
        let rule = JaNoSuccessiveWord::from_options(&serde_json::json!({"allowOnomatopee": false}));
        let diags = diagnose_with_morphology(&rule, "ドキドキ", |p, b| {
            noun(b, p, 0, 6);
            noun(b, p, 6, 12);
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("\"ドキ\""),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn allow_list_suppresses_specific_surface() {
        // allow:["の"] suppresses の×2.
        let rule = JaNoSuccessiveWord::from_options(&serde_json::json!({"allow": ["の"]}));
        let diags = diagnose_with_morphology(&rule, "のの", |p, b| {
            particle(b, p, 0, 3); // の
            particle(b, p, 3, 6); // の
        });
        assert!(diags.is_empty(), "{diags:?}");
        // Without allow, の×2 is flagged.
        let flagged = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "のの", |p, b| {
            particle(b, p, 0, 3);
            particle(b, p, 3, 6);
        });
        assert_eq!(flagged.len(), 1);
    }

    #[test]
    fn unknown_tokens_break_the_chain() {
        // An OOV (unknown) token between two identical surfaces resets the prev pointer.
        use tzlint_ast::morphology::Token;
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "私X私", |p, b| {
            noun(b, p, 0, 3); // 私
            b.push_token(
                TokenAttrs {
                    node: p,
                    surface: Span::new(3, 4),
                    lang: Lang::JA,
                    tagset: Tagset::IPADIC,
                    flags: Token::FLAG_UNKNOWN,
                },
                None,
                None,
                &[],
            ); // X (unknown)
            noun(b, p, 4, 7); // 私
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn particle_duplicate_is_flagged() {
        // の×2 (non-katakana, not a number) → flagged.
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "のの", |p, b| {
            particle(b, p, 0, 3); // の
            particle(b, p, 3, 6); // の
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn three_in_a_row_flags_the_second_and_third() {
        // A A A → report the 2nd and 3rd.
        let diags = diagnose_with_morphology(&JaNoSuccessiveWord::new(), "私私私", |p, b| {
            noun(b, p, 0, 3);
            noun(b, p, 3, 6);
            noun(b, p, 6, 9);
        });
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (3, 6));
        assert_eq!((diags[1].span.start, diags[1].span.end), (6, 9));
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        assert!(diagnose(&JaNoSuccessiveWord::new(), "私私").is_empty());
    }

    #[test]
    fn is_katakana_only_helper() {
        // Internal helper tests: cover all branches.
        assert!(is_katakana_only("ドキ"));
        assert!(is_katakana_only("ワヲン"));
        assert!(is_katakana_only("ー"));
        assert!(!is_katakana_only("")); // empty → false
        assert!(!is_katakana_only("ABC")); // ASCII
        assert!(!is_katakana_only("私")); // kanji
        assert!(!is_katakana_only("ドき")); // mixed hiragana
    }
}
