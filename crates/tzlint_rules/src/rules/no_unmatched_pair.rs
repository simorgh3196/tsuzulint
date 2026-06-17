//! `no-unmatched-pair` — flag unmatched bracket/quote pairs, faithfully mirroring upstream
//! `textlint-rule-no-unmatched-pair` by default.
//!
//! The matcher keeps a per-pair-*type* context (mirroring upstream's `SourceCode.contextLocations`
//! with `isInContext` / `enterContext` / `leaveContext`): an opening mark enters its type's
//! context, a closing mark of an already-open type leaves it, and any opener still open at the end
//! of the block is reported as unclosed. To match upstream exactly:
//!
//! - A closing mark with no open context of its type is **ignored** — upstream only ever reports
//!   unclosed *openers*, never a lone closer. This is deliberate: a lone closer is
//!   indistinguishable from an enumeration marker such as `1)` / `2)` / `a)`, which upstream's
//!   tests explicitly treat as valid.
//! - A second opening mark of an already-open type is **ignored** — the context is keyed by pair
//!   type, not a strict stack.
//! - Crossing nesting such as `([)]` is **not** flagged — each closer leaves its own type's
//!   context regardless of order, exactly as upstream does. (A stricter LIFO matcher would flag
//!   it, but that would diverge from textlint.)
//!
//! The 13 pairs mirror upstream `PairMaker.js`. The symmetric double quote `"` toggles (the first
//! occurrence opens, the second closes); the single quote `'` is intentionally not a pair,
//! following upstream.
//!
//! ## Option `detectOrphanedClosers` (default `false`)
//!
//! A Japanese-aware, opt-in extension *beyond* upstream parity. When enabled, an orphaned closing
//! mark (a closer whose type is not open) is **also** reported — but only for closers that never
//! double as a list/enumeration marker: the Japanese quotation/corner brackets `」』】〛` and the
//! guillemets `»›`. The enumeration-prone closers `) ） ] ］ } ｝` (and the symmetric `"`) are
//! never reported even when this is on, so `1)` / `2)` style markers stay false-positive-free. Off
//! by default to preserve byte-for-byte textlint parity.

use serde_json::Value;
use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-unmatched-pair";

/// A bracket/quote pair.
struct Pair {
    start: char,
    end: char,
    /// Japanese name shown in the diagnostic.
    name: &'static str,
    /// Whether an orphaned occurrence of `end` may be reported under `detectOrphanedClosers`.
    /// `true` only for closers that never act as a list/enumeration marker (Japanese
    /// quotation/corner brackets and guillemets); `false` for `) ） ] ］ } ｝` and `"`.
    flag_orphan: bool,
}

/// The pairs monitored by this rule, mirroring upstream `PairMaker.js` (13 pairs).
const PAIRS: &[Pair] = &[
    Pair {
        start: '"',
        end: '"',
        name: "二重引用符\"\"",
        flag_orphan: false,
    },
    Pair {
        start: '[',
        end: ']',
        name: "角括弧[]",
        flag_orphan: false,
    },
    Pair {
        start: '(',
        end: ')',
        name: "丸括弧()",
        flag_orphan: false,
    },
    Pair {
        start: '{',
        end: '}',
        name: "波括弧{}",
        flag_orphan: false,
    },
    Pair {
        start: '「',
        end: '」',
        name: "かぎ括弧「」",
        flag_orphan: true,
    },
    Pair {
        start: '（',
        end: '）',
        name: "全角丸括弧（）",
        flag_orphan: false,
    },
    Pair {
        start: '『',
        end: '』',
        name: "二重かぎ括弧『』",
        flag_orphan: true,
    },
    Pair {
        start: '｛',
        end: '｝',
        name: "全角波括弧｛｝",
        flag_orphan: false,
    },
    Pair {
        start: '［',
        end: '］',
        name: "全角角括弧［］",
        flag_orphan: false,
    },
    Pair {
        start: '〚',
        end: '〛',
        name: "二重角括弧〚〛",
        flag_orphan: true,
    },
    Pair {
        start: '【',
        end: '】',
        name: "隅付き括弧【】",
        flag_orphan: true,
    },
    Pair {
        start: '«',
        end: '»',
        name: "二重山括弧«»",
        flag_orphan: true,
    },
    Pair {
        start: '‹',
        end: '›',
        name: "山括弧‹›",
        flag_orphan: true,
    },
];

/// The index of the pair whose opener is `c`, if any.
fn pair_with_start(c: char) -> Option<usize> {
    PAIRS.iter().position(|p| p.start == c)
}

/// The index of the pair whose closer is `c`, if any.
fn pair_with_end(c: char) -> Option<usize> {
    PAIRS.iter().position(|p| p.end == c)
}

/// Flags unmatched bracket/quote pairs within a text block.
pub struct NoUnmatchedPair {
    meta: RuleMeta,
    /// Opt-in: also report orphaned Japanese quotation/corner-bracket closers (see module docs).
    detect_orphaned_closers: bool,
}

impl NoUnmatchedPair {
    /// Construct with defaults (textlint parity: `detect_orphaned_closers` off).
    pub fn new() -> Self {
        NoUnmatchedPair {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]).for_language(Lang::JA),
            detect_orphaned_closers: false,
        }
    }

    /// Construct from config `options`, leniently. Reads the boolean `detectOrphanedClosers`
    /// (default `false`); a missing or wrong-typed value keeps the default.
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(on) = options
            .get("detectOrphanedClosers")
            .and_then(Value::as_bool)
        {
            rule.detect_orphaned_closers = on;
        }
        rule
    }
}

impl Default for NoUnmatchedPair {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoUnmatchedPair {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();

        // Per-pair-type context: `(PAIRS index, byte offset of the opener)`. At most one entry per
        // type, mirroring upstream's `isInContext` / `enterContext` / `leaveContext`.
        let mut context: Vec<(usize, u32)> = Vec::new();

        for (i, c) in text.char_indices() {
            let start = pair_with_start(c);
            let end = pair_with_end(c);
            // The pair type this character belongs to (a symmetric `"` is both a start and an end).
            let key = match start.or(end) {
                Some(key) => key,
                None => continue, // not a pair mark
            };

            if context.iter().any(|(k, _)| *k == key) {
                // This type is already open: a closing mark leaves the context; a second opening
                // mark of the same type is ignored.
                if let Some(pos) =
                    end.and_then(|end_key| context.iter().position(|(k, _)| *k == end_key))
                {
                    context.remove(pos);
                }
            } else if let Some(start_key) = start {
                // A fresh opening mark enters the context.
                context.push((start_key, base.saturating_add(i as u32)));
            } else {
                // A closing mark whose type is not open (orphaned). Ignored by default (upstream
                // parity); under `detectOrphanedClosers`, reported only for non-enumeration closers.
                let orphan = end
                    .filter(|_| self.detect_orphaned_closers)
                    .and_then(|end_key| PAIRS.get(end_key))
                    .filter(|pair| pair.flag_orphan);
                if let Some(pair) = orphan {
                    let offset = base.saturating_add(i as u32);
                    cx.report(
                        Span::new(offset, offset.saturating_add(c.len_utf8() as u32)),
                        format!("「{}」に対応する開き括弧がありません（{}）。", c, pair.name),
                    );
                }
            }
        }

        // Any opener still open at the end of the block is unclosed.
        for (pair_idx, offset) in context {
            if let Some(pair) = PAIRS.get(pair_idx) {
                cx.report(
                    Span::new(offset, offset.saturating_add(pair.start.len_utf8() as u32)),
                    format!(
                        "「{}」に対応する閉じ括弧がありません（{}）。",
                        pair.start, pair.name
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    /// The rule with `detectOrphanedClosers` enabled.
    fn strict() -> NoUnmatchedPair {
        NoUnmatchedPair::from_options(&serde_json::json!({ "detectOrphanedClosers": true }))
    }

    // --- matched pairs pass ---

    #[test]
    fn matched_ascii_parens_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "これは(秘密)です。\n").is_empty());
    }

    #[test]
    fn matched_ja_parens_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "これは（秘密）です。\n").is_empty());
    }

    #[test]
    fn matched_kagi_brackets_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "「こんにちは」と言った。\n").is_empty());
    }

    #[test]
    fn nested_matching_pairs_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "（これは「秘密」です）。\n").is_empty());
    }

    #[test]
    fn matched_double_quote_passes() {
        assert!(diagnose(&NoUnmatchedPair::new(), "He said \"hello\".\n").is_empty());
    }

    #[test]
    fn matched_square_brackets_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "See [here] for details.\n").is_empty());
    }

    #[test]
    fn guillemet_pairs_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "«hello»\n").is_empty());
        assert!(diagnose(&NoUnmatchedPair::new(), "‹world›\n").is_empty());
    }

    // --- unclosed openers are flagged (both modes) ---

    #[test]
    fn unclosed_ja_opener_is_flagged() {
        let diags = diagnose(&NoUnmatchedPair::new(), "これは（秘密です。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("閉じ括弧がありません"));
    }

    #[test]
    fn unclosed_square_bracket_opener_is_flagged() {
        assert_eq!(
            diagnose(&NoUnmatchedPair::new(), "See [here for details.\n").len(),
            1
        );
    }

    #[test]
    fn unclosed_double_quote_opener_is_flagged() {
        assert_eq!(
            diagnose(&NoUnmatchedPair::new(), "He said \"hello.\n").len(),
            1
        );
    }

    // --- default (parity) mode: cases that are intentionally NOT flagged ---

    #[test]
    fn lone_closer_is_ignored_by_default() {
        // Upstream reports only unclosed openers; a closing mark with no open context of its type
        // is ignored. A lone ） must not be flagged in the default (parity) mode.
        assert!(diagnose(&NoUnmatchedPair::new(), "秘密）です。\n").is_empty());
    }

    #[test]
    fn lone_ja_quote_closer_is_ignored_by_default() {
        assert!(diagnose(&NoUnmatchedPair::new(), "重要」です。\n").is_empty());
    }

    #[test]
    fn crossing_nesting_is_not_flagged() {
        // `（「…）」` crosses the two pairs. Upstream's per-type context closes each pair
        // independently of order, so crossing nesting is intentionally not flagged in either mode.
        assert!(diagnose(&NoUnmatchedPair::new(), "（「秘密）」です。\n").is_empty());
        assert!(diagnose(&strict(), "（「秘密）」です。\n").is_empty());
    }

    #[test]
    fn second_opener_of_same_type_is_ignored() {
        // The context is keyed by pair type, so a second （ while round is already open is ignored;
        // a single ） then closes it and nothing is reported (mirrors upstream).
        assert!(diagnose(&NoUnmatchedPair::new(), "（（秘密）です。\n").is_empty());
    }

    #[test]
    fn ascii_closer_does_not_close_fullwidth_opener() {
        // The full-width （ stays open (a different type from the ASCII )), and the ASCII ) is a
        // lone closer that is ignored — only the unclosed （ is reported.
        let diags = diagnose(&NoUnmatchedPair::new(), "（秘密)\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    // --- opt-in detectOrphanedClosers mode ---

    #[test]
    fn orphaned_ja_quote_closer_flagged_when_enabled() {
        // A lone 」 cannot be an enumeration marker, so the strict mode reports it.
        let diags = diagnose(&strict(), "重要」です。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("開き括弧がありません"));
    }

    #[test]
    fn orphaned_corner_bracket_closer_flagged_when_enabled() {
        let diags = diagnose(&strict(), "注意】を確認する。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn enumeration_markers_not_flagged_even_when_enabled() {
        // ) is enumeration-prone, so it is never reported as an orphan even in the strict mode.
        assert!(diagnose(&strict(), "手順は1) 準備 2) 実行 3) 確認 です。\n").is_empty());
        assert!(diagnose(&strict(), "選択肢は（1）と2）があります。\n").is_empty());
    }

    #[test]
    fn matched_quotes_still_pass_when_enabled() {
        assert!(diagnose(&strict(), "彼は「了解」と言った。\n").is_empty());
    }
}
