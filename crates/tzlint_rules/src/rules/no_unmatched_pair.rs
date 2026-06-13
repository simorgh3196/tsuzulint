//! `no-unmatched-pair` — flag unmatched bracket/paren pairs in prose text.
//!
//! Mirrors the upstream textlint rule of the same name. Scans each text block for
//! the pairs listed in `PAIRS` using a stack-based approach: openers push onto the stack,
//! matching closers pop; at end-of-block any remaining open openers are reported, and
//! any closer with no matching opener on the stack is reported immediately.
//!
//! Pair list mirrors PairMaker.js from the upstream repo (13 pairs; symmetric quotes
//! like `'` and `"` single-char are omitted following upstream).

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-unmatched-pair";

/// A bracket pair: opener, closer, and human-readable name for the diagnostic.
struct Pair {
    start: char,
    end: char,
    /// Japanese/English name used in the error message.
    name: &'static str,
}

/// All pairs monitored by this rule, mirroring PairMaker.js.
const PAIRS: &[Pair] = &[
    Pair {
        start: '"',
        end: '"',
        name: "二重引用符\"\"",
    },
    Pair {
        start: '[',
        end: ']',
        name: "角括弧[]",
    },
    Pair {
        start: '(',
        end: ')',
        name: "丸括弧()",
    },
    Pair {
        start: '{',
        end: '}',
        name: "波括弧{}",
    },
    Pair {
        start: '「',
        end: '」',
        name: "かぎ括弧「」",
    },
    Pair {
        start: '（',
        end: '）',
        name: "全角丸括弧（）",
    },
    Pair {
        start: '『',
        end: '』',
        name: "二重かぎ括弧『』",
    },
    Pair {
        start: '｛',
        end: '｝',
        name: "全角波括弧｛｝",
    },
    Pair {
        start: '［',
        end: '］',
        name: "全角角括弧［］",
    },
    Pair {
        start: '〚',
        end: '〛',
        name: "二重角括弧〚〛",
    },
    Pair {
        start: '【',
        end: '】',
        name: "隅付き括弧【】",
    },
    Pair {
        start: '«',
        end: '»',
        name: "二重山括弧«»",
    },
    Pair {
        start: '‹',
        end: '›',
        name: "山括弧‹›",
    },
];

/// Find the pair definition whose `end` matches `c`, if any.
fn pair_for_end(c: char) -> Option<&'static Pair> {
    PAIRS.iter().find(|p| p.end == c)
}

/// True if `c` is a start character for any pair.
fn is_start(c: char) -> bool {
    PAIRS.iter().any(|p| p.start == c)
}

/// True if `c` is an end character for any pair.
fn is_end(c: char) -> bool {
    PAIRS.iter().any(|p| p.end == c)
}

/// True if `c` is both a start and an end of the same pair (symmetric, e.g. `"`
/// if we ever added it). For all current pairs this is false, but the check
/// keeps the logic correct if pairs are extended.
fn is_symmetric(c: char) -> bool {
    PAIRS.iter().any(|p| p.start == p.end && p.start == c)
}

/// Flags unmatched bracket/paren pairs within text blocks.
pub struct NoUnmatchedPair {
    meta: RuleMeta,
}

impl NoUnmatchedPair {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoUnmatchedPair {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]).for_language(Lang::JA),
        }
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

        // Stack entries: (byte_offset_of_char, pair_index_in_PAIRS)
        let mut stack: Vec<(usize, usize)> = Vec::new();

        for (i, c) in text.char_indices() {
            let char_end = i + c.len_utf8();

            if is_symmetric(c) {
                // Symmetric pair: toggle — if currently open, close; else open.
                if let Some(pos) = stack.iter().rposition(|(_, pi)| PAIRS[*pi].start == c) {
                    stack.remove(pos);
                } else if let Some(pair_idx) = PAIRS.iter().position(|p| p.start == c) {
                    stack.push((i, pair_idx));
                }
            } else if is_start(c) && !is_end(c) {
                // Pure opener
                if let Some(pair_idx) = PAIRS.iter().position(|p| p.start == c) {
                    stack.push((i, pair_idx));
                }
            } else if is_end(c) && !is_start(c) {
                // Pure closer
                if let Some(pair) = pair_for_end(c) {
                    // Find the most recent matching opener on the stack.
                    if let Some(pos) = stack
                        .iter()
                        .rposition(|(_, pi)| PAIRS[*pi].start == pair.start)
                    {
                        stack.remove(pos);
                    } else {
                        // Closer with no matching opener.
                        cx.report(
                            Span::new(
                                base.saturating_add(i as u32),
                                base.saturating_add(char_end as u32),
                            ),
                            format!("「{}」に対応する開き括弧がありません（{}）。", c, pair.name),
                        );
                    }
                }
            }
        }

        // Any remaining openers on the stack are unmatched.
        for (open_i, pair_idx) in stack {
            let pair = &PAIRS[pair_idx];
            let open_char = pair.start;
            let open_end = open_i + open_char.len_utf8();
            cx.report(
                Span::new(
                    base.saturating_add(open_i as u32),
                    base.saturating_add(open_end as u32),
                ),
                format!(
                    "「{}」に対応する閉じ括弧がありません（{}）。",
                    open_char, pair.name
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

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
    fn unmatched_opener_is_flagged() {
        let diags = diagnose(&NoUnmatchedPair::new(), "これは（秘密です。\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("閉じ括弧がありません"));
    }

    #[test]
    fn unmatched_closer_is_flagged() {
        let diags = diagnose(&NoUnmatchedPair::new(), "秘密）です。\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("開き括弧がありません"));
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
    fn unmatched_double_quote_opener_is_flagged() {
        let diags = diagnose(&NoUnmatchedPair::new(), "He said \"hello.\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn matched_square_brackets_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "See [here] for details.\n").is_empty());
    }

    #[test]
    fn unmatched_square_bracket_opener_is_flagged() {
        let diags = diagnose(&NoUnmatchedPair::new(), "See [here for details.\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn mixed_ja_ascii_brackets_each_match_own_kind() {
        // （ matched by ） — ok
        assert!(diagnose(&NoUnmatchedPair::new(), "（秘密）\n").is_empty());
        // （ not closed — ng
        let diags = diagnose(&NoUnmatchedPair::new(), "（秘密)\n");
        // ASCII ) does not match（; the ) is an unmatched closer and （ is an unmatched opener
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn guillemet_pairs_pass() {
        assert!(diagnose(&NoUnmatchedPair::new(), "«hello»\n").is_empty());
        assert!(diagnose(&NoUnmatchedPair::new(), "‹world›\n").is_empty());
    }
}
