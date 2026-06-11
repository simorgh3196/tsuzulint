//! `ja-no-redundant-expression` — flag a small, high-confidence set of redundant Japanese
//! expressions (冗長表現) that a tighter form expresses just as well.
//!
//! A **morphology-dependent** rule (Japanese). 0.1.0 ships the canonical「〜することができる」family:
//! the token run `こと` (名詞) → `が` (格助詞) → `できる` (動詞) or `可能` (名詞), which reads more
//! tightly as 「〜できる」. It is **report-only** (rewriting needs verb conjugation — 泳ぐことができる →
//! 泳げる — which is unsafe to automate). The pattern set is intentionally narrow and extensible;
//! it keys on IPADIC POS literals, so an unrecognized tagset matches nothing (a false negative,
//! never a false positive — mirrors `no-doubled-joshi`).

use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_ast::{ArchivedAst, NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-no-redundant-expression";

/// Flags a redundant「〜することができる」-family expression.
pub struct JaNoRedundantExpression {
    meta: RuleMeta,
}

impl JaNoRedundantExpression {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        JaNoRedundantExpression {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
        }
    }
}

impl Default for JaNoRedundantExpression {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaNoRedundantExpression {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let Some(table) = cx.morphology() else {
            return; // belt-and-suspenders over the engine's morphology gate
        };
        let ast = cx.ast();

        let mut tokens: Vec<&ArchivedToken> = cx.tokens_of(node.id()).collect();
        tokens.sort_by_key(|t| (t.surface().start, t.surface().end));

        // Scan for the こと + が + (できる | 可能) run on adjacent tokens.
        for window in tokens.windows(3) {
            let [koto, ga, tail] = window else {
                continue;
            };
            if koto.is_unknown() || ga.is_unknown() || tail.is_unknown() {
                continue;
            }
            let is_koto = pos(koto, table) == Some("名詞") && surface(koto, ast) == Some("こと");
            let is_ga = pos(ga, table) == Some("助詞")
                && sub1(ga, table) == Some("格助詞")
                && surface(ga, ast) == Some("が");
            let is_tail = (pos(tail, table) == Some("動詞")
                && tail.base_form(table) == Some("できる"))
                || (pos(tail, table) == Some("名詞") && surface(tail, ast) == Some("可能"));
            if is_koto && is_ga && is_tail {
                cx.report(Span::new(koto.surface().start, tail.surface().end), MESSAGE);
            }
        }
    }
}

/// The Japanese diagnostic for a「〜することができる」-family redundancy.
const MESSAGE: &str = "冗長な表現「ことができる」がみつかりました。\n\n\
「〜できる」と簡潔に書けないか検討してください（例: 泳ぐことができる → 泳げる）。";

fn pos<'a>(tok: &ArchivedToken, table: &'a ArchivedMorphologyV1) -> Option<&'a str> {
    feature(tok, table, FeatureKey::POS)
}
fn sub1<'a>(tok: &ArchivedToken, table: &'a ArchivedMorphologyV1) -> Option<&'a str> {
    feature(tok, table, FeatureKey::POS_SUB_1)
}
fn surface<'a>(tok: &ArchivedToken, ast: &'a ArchivedAst) -> Option<&'a str> {
    ast.text_of(tok.surface())
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

    fn verb(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32, base: &str) {
        push(
            b,
            node,
            start,
            end,
            Some(base),
            &[(FeatureKey::POS, "動詞")],
        );
    }
    fn koto(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, None, &[(FeatureKey::POS, "名詞")]);
    }
    fn case_ga(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(
            b,
            node,
            start,
            end,
            None,
            &[(FeatureKey::POS, "助詞"), (FeatureKey::POS_SUB_1, "格助詞")],
        );
    }
    fn kanou(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        push(b, node, start, end, None, &[(FeatureKey::POS, "名詞")]);
    }

    #[test]
    fn flags_koto_ga_dekiru() {
        // 泳ぐことができる — こと(6..12) が(12..15) できる(15..24) → flag こと..できる (6..24).
        let diags = diagnose_with_morphology(
            &JaNoRedundantExpression::new(),
            "泳ぐことができる",
            |p, b| {
                verb(b, p, 0, 6, "泳ぐ"); // 泳ぐ
                koto(b, p, 6, 12); // こと
                case_ga(b, p, 12, 15); // が
                verb(b, p, 15, 24, "できる"); // できる
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (6, 24));
        assert!(
            diags[0].message.contains("ことができる"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_koto_ga_kanou() {
        // 利用することが可能 — こと(12..18) が(18..21) 可能(21..27) → flag こと..可能.
        let diags = diagnose_with_morphology(
            &JaNoRedundantExpression::new(),
            "利用することが可能",
            |p, b| {
                koto(b, p, 0, 6); // 利用 (treated as 名詞 here)
                verb(b, p, 6, 12, "する"); // する
                koto(b, p, 12, 18); // こと
                case_ga(b, p, 18, 21); // が
                kanou(b, p, 21, 27); // 可能
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (12, 27));
    }

    #[test]
    fn a_lone_koto_is_clean() {
        // ことが大事 — こと + が + 大事(not できる/可能) → not a redundant run.
        let diags = diagnose_with_morphology(
            &JaNoRedundantExpression::new(),
            "ことが大事",
            |p, b| {
                koto(b, p, 0, 6); // こと
                case_ga(b, p, 6, 9); // が
                kanou(b, p, 9, 15); // 大事 (名詞, surface != 可能)
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn subject_koto_then_dekiru_without_ga_is_clean() {
        // こと を できる (を, not が) → no match.
        let diags = diagnose_with_morphology(
            &JaNoRedundantExpression::new(),
            "ことをできる",
            |p, b| {
                koto(b, p, 0, 6); // こと
                push(
                    b,
                    p,
                    6,
                    9,
                    None,
                    &[(FeatureKey::POS, "助詞"), (FeatureKey::POS_SUB_1, "格助詞")],
                ); // を (格助詞 but surface を)
                verb(b, p, 9, 18, "できる"); // できる
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        assert!(diagnose(&JaNoRedundantExpression::new(), "泳ぐことができる").is_empty());
    }
}
