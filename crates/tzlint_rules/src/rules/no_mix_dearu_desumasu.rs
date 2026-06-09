//! `no-mix-dearu-desumasu` — flag mixing the である (plain/literary) and ですます (polite)
//! sentence styles (文体) within one document.
//!
//! A **morphology-dependent**, **document-level** rule. It declares
//! [`with_morphology`](RuleMeta::with_morphology) for Japanese (so the engine runs it only when a
//! morphology table is available, and R5 scopes it to JA) and visits the prose block kinds purely
//! so the engine **tokenizes** them; the real work is in [`finish`](Rule::finish), which re-walks
//! the document, classifies each sentence's final predicate from its tokens, then flags the
//! minority style across the whole document.
//!
//! **Classification** scans a sentence's tokens from the end, skipping sentence-final particles
//! and non-determining auxiliaries (た/ない/…), and decides on the first style-bearing token: a
//! 助動詞 whose base form is です/ます ⇒ ですます; だ/である ⇒ である; a plain 動詞/形容詞 ⇒ である;
//! a 名詞 with no copula (体言止め) ⇒ unclassified (skipped). It keys on **IPADIC** POS literals,
//! so an unrecognized tagset simply yields no classification — a false negative, never a false
//! positive (mirrors `no-doubled-joshi`).

use std::collections::BTreeMap;

use serde_json::Value;
use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-mix-dearu-desumasu";

/// Sentence terminators that end a sentence for style classification.
const TERMINATORS: [char; 7] = ['。', '．', '.', '！', '？', '!', '?'];

/// A Japanese sentence style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// である体 (plain / literary).
    Dearu,
    /// ですます体 (polite).
    Desumasu,
}

/// Flags sentences whose style is the document minority (or, with `prefer`, not the chosen style).
pub struct NoMixDearuDesumasu {
    meta: RuleMeta,
    /// The style to enforce. When set, every sentence not in this style is flagged regardless of
    /// counts; when `None`, the document majority wins and the minority is flagged.
    prefer: Option<Style>,
}

impl NoMixDearuDesumasu {
    /// Construct with the default options (auto-detect the majority; no forced preference).
    pub fn new() -> Self {
        NoMixDearuDesumasu {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
            prefer: None,
        }
    }

    /// Construct from config `options`, leniently: `prefer` is `"dearu"` or `"desumasu"`; any other
    /// value (or absent) leaves the rule in auto-detect mode.
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        rule.prefer = match options.get("prefer").and_then(Value::as_str) {
            Some("dearu") => Some(Style::Dearu),
            Some("desumasu") => Some(Style::Desumasu),
            _ => None,
        };
        rule
    }
}

impl Default for NoMixDearuDesumasu {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoMixDearuDesumasu {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    // Per-node work is unnecessary: the node kinds are declared only so the engine tokenizes the
    // prose blocks. Document-wide classification happens once in `finish`.
    fn check<'ast>(&self, _node: NodeRef<'ast>, _cx: &mut Context<'ast>) {}

    fn finish<'ast>(&self, cx: &mut Context<'ast>) {
        let Some(table) = cx.morphology() else {
            return; // belt-and-suspenders over the engine's morphology gate
        };
        let ast = cx.ast();
        let Some(root) = NodeRef::root(ast) else {
            return;
        };

        // Document-wide: collect each classifiable sentence's (style, determining-token span) via a
        // cycle-safe walk. A classifiable block is self-contained, so its subtree is not descended.
        let mut sentences: Vec<(Style, Span)> = Vec::new();
        let mut visited = vec![false; ast.len()];
        if let Some(slot) = visited.get_mut(root.id().0 as usize) {
            *slot = true;
        }
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if matches!(
                node.kind(),
                NodeKind::PARAGRAPH | NodeKind::HEADING | NodeKind::TABLE_CELL
            ) {
                classify_block(node, cx, table, &mut sentences);
                continue;
            }
            for child in node.children() {
                if let Some(slot) = visited.get_mut(child.id().0 as usize)
                    && !*slot
                {
                    *slot = true;
                    stack.push(child);
                }
            }
        }

        // Decide which style is the violation set.
        let dearu = sentences.iter().filter(|(s, _)| *s == Style::Dearu).count();
        let desumasu = sentences.len() - dearu;
        let flagged = match self.prefer {
            // A forced preference flags anything not in it — even a single-style document.
            Some(Style::Dearu) => Style::Desumasu,
            Some(Style::Desumasu) => Style::Dearu,
            None => {
                // Auto-detect: only a genuine mix is a violation; flag the minority. On a tie, the
                // である sentences are flagged (ですます is treated as the default majority).
                if dearu == 0 || desumasu == 0 {
                    return;
                }
                if desumasu >= dearu {
                    Style::Dearu
                } else {
                    Style::Desumasu
                }
            }
        };

        let message = mixed_style_message(flagged);
        for (style, span) in sentences {
            if style == flagged {
                cx.report(span, message.as_str());
            }
        }
    }
}

/// Classify every sentence in one prose block (split on [`TERMINATORS`]) and append each
/// classifiable sentence's `(style, determining-token span)` to `out`.
fn classify_block<'ast>(
    node: NodeRef<'ast>,
    cx: &Context<'ast>,
    table: &'ast ArchivedMorphologyV1,
    out: &mut Vec<(Style, Span)>,
) {
    let base = node.span().start;
    let text = node.text();
    let mut tokens: Vec<&ArchivedToken> = cx.tokens_of(node.id()).collect();
    // The `(node, surface.start)` emission order is a producer contract, not enforced — sort.
    tokens.sort_by_key(|t| (t.surface().start, t.surface().end));

    // Sentence index of a byte offset = the number of terminators before it within the block.
    let sentence_idx = |start: u32| -> usize {
        let upto = start.saturating_sub(base) as usize;
        text.get(..upto)
            .unwrap_or("")
            .chars()
            .filter(|c| TERMINATORS.contains(c))
            .count()
    };

    // Group source-ordered tokens by sentence, then classify each sentence's final predicate.
    let mut groups: BTreeMap<usize, Vec<&ArchivedToken>> = BTreeMap::new();
    for &t in &tokens {
        groups
            .entry(sentence_idx(t.surface().start))
            .or_default()
            .push(t);
    }
    for group in groups.values() {
        if let Some(styled) = classify_sentence(group, table) {
            out.push(styled);
        }
    }
}

/// Classify a sentence from its source-ordered `tokens` by its final predicate, returning the
/// [`Style`] and the span of the style-determining token. `None` for an unclassifiable sentence
/// (体言止め, a fragment, or an unrecognized tagset whose POS literals do not match).
///
/// Scans from the end: sentence-final particles, symbols, and non-determining auxiliaries (た /
/// ない / う / …) are skipped; the first style-bearing token decides — a 助動詞 with base です/ます
/// ⇒ ですます, だ/である ⇒ である, a plain 動詞/形容詞 ⇒ である, a 名詞 with no copula ⇒ none.
fn classify_sentence(
    tokens: &[&ArchivedToken],
    table: &ArchivedMorphologyV1,
) -> Option<(Style, Span)> {
    for &tok in tokens.iter().rev() {
        if tok.is_unknown() {
            continue; // an OOV guess is not trusted (mirrors no-doubled-joshi)
        }
        match feature(tok, table, FeatureKey::POS) {
            Some("助動詞") => match tok.base_form(table) {
                Some("です") | Some("ます") => return Some((Style::Desumasu, tok.surface())),
                Some("だ") | Some("である") => return Some((Style::Dearu, tok.surface())),
                // た / ない / う / ん / … don't fix politeness — keep scanning back.
                _ => continue,
            },
            // A plain verbal / adjectival predicate is である体.
            Some("動詞") | Some("形容詞") => return Some((Style::Dearu, tok.surface())),
            // A nominal ending with no copula is 体言止め — a style we do not classify.
            Some("名詞") | Some("代名詞") | Some("形状詞") => return None,
            // Particles, symbols, prefixes, …: skip and keep looking for the predicate.
            _ => continue,
        }
    }
    None
}

/// The Japanese diagnostic for a sentence in the `flagged` (violating) style; it names the style
/// to unify toward (the other one — the document majority, or the `prefer`red style).
fn mixed_style_message(flagged: Style) -> String {
    let (flagged_name, target_name) = match flagged {
        Style::Dearu => ("である", "ですます"),
        Style::Desumasu => ("ですます", "である"),
    };
    format!(
        "文体が混在しています。この文は「{flagged_name}」体ですが、文書全体では「{target_name}」体に統一してください。"
    )
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
    use tzlint_ast::NodeId;
    use tzlint_ast::morphology::{MorphologyBuilder, Tagset, TokenAttrs};

    /// Push a token with major POS `pos` and optional `base` form at `[start, end)`.
    fn tok(
        b: &mut MorphologyBuilder,
        node: NodeId,
        start: u32,
        end: u32,
        pos: &str,
        base: Option<&str>,
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
            &[(FeatureKey::POS, pos)],
        );
    }

    fn noun(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        tok(b, node, start, end, "名詞", None);
    }
    /// A 助動詞 (auxiliary) with the given base form (です / ます / だ / た / ない / …).
    fn aux(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32, base: &str) {
        tok(b, node, start, end, "助動詞", Some(base));
    }
    fn verb(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32, base: &str) {
        tok(b, node, start, end, "動詞", Some(base));
    }
    fn particle(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        tok(b, node, start, end, "助詞", None);
    }

    #[test]
    fn flags_the_minority_dearu_sentence() {
        // 猫です。犬です。鳥だ。 — two ですます, one である → flag the lone である sentence at だ (27..30).
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "猫です。犬です。鳥だ。",
            |p, b| {
                noun(b, p, 0, 3); // 猫
                aux(b, p, 3, 9, "です"); // です
                noun(b, p, 12, 15); // 犬   (。 9..12)
                aux(b, p, 15, 21, "です"); // です
                noun(b, p, 24, 27); // 鳥   (。 21..24)
                aux(b, p, 27, 30, "だ"); // だ   (。 30..33)
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (27, 30));
    }

    #[test]
    fn flags_the_minority_desumasu_sentence() {
        // 猫だ。犬だ。鳥です。 — two である, one ですます → flag です (21..27).
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "猫だ。犬だ。鳥です。",
            |p, b| {
                noun(b, p, 0, 3); // 猫
                aux(b, p, 3, 6, "だ"); // だ   (。 6..9)
                noun(b, p, 9, 12); // 犬
                aux(b, p, 12, 15, "だ"); // だ  (。 15..18)
                noun(b, p, 18, 21); // 鳥
                aux(b, p, 21, 27, "です"); // です (。 27..30)
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (21, 27));
        assert!(
            diags[0].message.contains("ですます"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn a_single_style_document_is_clean() {
        // All ですます → no mix → no diagnostics.
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "猫です。犬です。",
            |p, b| {
                noun(b, p, 0, 3);
                aux(b, p, 3, 9, "です");
                noun(b, p, 12, 15);
                aux(b, p, 15, 21, "です");
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn taigendome_sentences_are_not_classified() {
        // 猫です。犬です。鳥。 — the noun-ending (体言止め) third sentence is skipped, leaving a single
        // style → clean. (If 鳥 were misclassified as である, we'd see a flagged mix.)
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "猫です。犬です。鳥。",
            |p, b| {
                noun(b, p, 0, 3);
                aux(b, p, 3, 9, "です");
                noun(b, p, 12, 15); // 犬 (。 9..12)
                aux(b, p, 15, 21, "です");
                noun(b, p, 24, 27); // 鳥 (。 21..24) — bare noun, no copula → skipped
            },
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn past_tense_is_classified_by_the_auxiliary_before_ta() {
        // 猫でした。犬だった。 — でした=ですます (です before た), だった=である (だ before た) → tie → flag
        // である (だっ at 18..21).
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "猫でした。犬だった。",
            |p, b| {
                noun(b, p, 0, 3); // 猫
                aux(b, p, 3, 9, "です"); // でし
                aux(b, p, 9, 12, "た"); // た   (。 12..15)
                noun(b, p, 15, 18); // 犬
                aux(b, p, 18, 21, "だ"); // だっ
                aux(b, p, 21, 24, "た"); // た   (。 24..27)
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (18, 21));
    }

    #[test]
    fn plain_verb_is_dearu_and_masu_is_desumasu() {
        // 歩く。走ります。 — 歩く (plain 動詞)=である, 走ります (ます)=ですます → tie → flag である (歩く).
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "歩く。走ります。",
            |p, b| {
                verb(b, p, 0, 6, "歩く"); // 歩く  (。 6..9)
                verb(b, p, 9, 15, "走る"); // 走り
                aux(b, p, 15, 21, "ます"); // ます  (。 21..24)
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (0, 6));
    }

    #[test]
    fn a_trailing_question_particle_is_skipped() {
        // 行きますか。帰る。 — 〜ますか=ですます (か skipped, ます found), 帰る=である → tie → flag である.
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "行きますか。帰る。",
            |p, b| {
                verb(b, p, 0, 6, "行く"); // 行き
                aux(b, p, 6, 12, "ます"); // ます
                particle(b, p, 12, 15); // か   (。 15..18)
                verb(b, p, 18, 24, "帰る"); // 帰る  (。 24..27)
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (18, 24));
    }

    #[test]
    fn prefer_dearu_flags_every_desumasu_sentence() {
        // prefer:"dearu" flags ですます even when it is the majority.
        let rule = NoMixDearuDesumasu::from_options(&serde_json::json!({ "prefer": "dearu" }));
        let diags = diagnose_with_morphology(&rule, "猫です。犬だ。", |p, b| {
            noun(b, p, 0, 3);
            aux(b, p, 3, 9, "です"); // です (。 9..12)
            noun(b, p, 12, 15);
            aux(b, p, 15, 18, "だ"); // だ
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (3, 9)); // the です sentence
    }

    #[test]
    fn a_tie_flags_the_dearu_sentences() {
        // One of each, no prefer → tie → である is flagged (ですます treated as the default majority).
        let diags = diagnose_with_morphology(
            &NoMixDearuDesumasu::new(),
            "猫です。犬だ。",
            |p, b| {
                noun(b, p, 0, 3);
                aux(b, p, 3, 9, "です"); // です (。 9..12)
                noun(b, p, 12, 15);
                aux(b, p, 15, 18, "だ"); // だ
            },
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (15, 18)); // the だ sentence
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        // In production with no JA provider injected, the engine passes no table → rule skipped.
        assert!(diagnose(&NoMixDearuDesumasu::new(), "猫です。犬だ。").is_empty());
    }

    #[test]
    fn an_unrecognized_predicate_pos_yields_no_classification() {
        // 猫です。あ。 — the second sentence's only token is a 感動詞 (not a predicate POS we match) →
        // unclassified, leaving a single style → clean. A false negative, never a false positive.
        let diags =
            diagnose_with_morphology(&NoMixDearuDesumasu::new(), "猫です。あ。", |p, b| {
                noun(b, p, 0, 3);
                aux(b, p, 3, 9, "です"); // です (。 9..12)
                tok(b, p, 12, 15, "感動詞", None); // あ (。 15..18)
            });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn build_rule_routes_the_prefer_option() {
        use crate::build_rule;
        // prefer:"desumasu" reaches the constructed rule and flags the である sentence.
        let rule = build_rule(ID, &serde_json::json!({ "prefer": "desumasu" }), None).unwrap();
        let diags = diagnose_with_morphology(rule.as_ref(), "猫です。犬だ。", |p, b| {
            noun(b, p, 0, 3);
            aux(b, p, 3, 9, "です");
            noun(b, p, 12, 15);
            aux(b, p, 15, 18, "だ"); // だ
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (15, 18)); // the だ sentence
    }
}
