//! `ja-no-weak-phrase` — flag weak or uncertain expressions (弱い表現) that undermine technical
//! writing confidence. Ported from `textlint-ja/textlint-rule-ja-no-weak-phrase`.
//!
//! A **surface** rule (Japanese). Upstream uses morpheme-token matching; this port implements the
//! same intent via exact substring search on the text of each inline node. The dictionary covers
//! the full upstream set of 5 patterns:
//!
//! 1. `かも。` — hedge + sentence terminator (副助詞「かも」直後に句点)
//! 2. `かもしれ` — 「かもしれない / かもしれません」family
//! 3. `思う` — 思う base form (e.g. 「〜と思う」)
//! 4. `思います` — polite form (「〜と思います」)
//! 5. `可能性を示唆している` — "suggests a possibility" (double-hedging)
//!
//! Surface search is conservative: every flagged substring is a genuine weak expression in
//! Japanese technical writing. Report-only (no autofix).

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-no-weak-phrase";

/// A single dictionary entry: the substring to match and the Japanese diagnostic message.
struct Entry {
    /// The surface substring (UTF-8) to search for.
    phrase: &'static str,
    /// Japanese diagnostic message.
    message: &'static str,
}

/// The upstream dictionary, ported faithfully. 5 entries total.
static DICT: &[Entry] = &[
    // Entry 1: かも + 句点 (sentence-final hedge)
    Entry {
        phrase: "かも。",
        message: "弱い表現「かも」が使われています。断言できる表現に書き換えてください。",
    },
    // Entry 2: かもしれ (かもしれない / かもしれません)
    Entry {
        phrase: "かもしれ",
        message: "弱い表現「かもしれ」が使われています。断言できる表現に書き換えてください。",
    },
    // Entry 3: 思う (base form) — 「〜と思う」
    Entry {
        phrase: "思う",
        message: "弱い表現「思う」が使われています。断言できる表現に書き換えてください。",
    },
    // Entry 4: 思います (polite form)
    Entry {
        phrase: "思います",
        message: "弱い表現「思います」が使われています。断言できる表現に書き換えてください。",
    },
    // Entry 5: 可能性を示唆している — double-hedging expression
    Entry {
        phrase: "可能性を示唆している",
        message: "弱い表現「可能性を示唆している」が使われています。\
「可能性がある」または「…を示唆している」を利用してください。\
弱い表現を二つ重ねることはしないでください。",
    },
];

/// Flags weak/uncertain expressions in Japanese technical writing.
pub struct JaNoWeakPhrase {
    meta: RuleMeta,
}

impl JaNoWeakPhrase {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        JaNoWeakPhrase {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
        }
    }
}

impl Default for JaNoWeakPhrase {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaNoWeakPhrase {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();

        // For each dictionary entry, report every non-overlapping occurrence in the node text.
        // Entries 3 (思う) and 4 (思います) share a prefix; we must avoid double-reporting the
        // 思います case as also 思う. We handle this by checking longer phrases first and skipping
        // the shorter "思う" match when "思います" already covers the same position.
        //
        // Strategy: collect all (byte_start, byte_end, message) hits for all entries, then emit
        // only those not already covered by a longer match that starts at the same position.
        let mut hits: Vec<(usize, usize, &'static str)> = Vec::new();

        for entry in DICT {
            let phrase_bytes = entry.phrase.len();
            let mut search_from = 0usize;
            while let Some(rel) = text[search_from..].find(entry.phrase) {
                let abs = search_from + rel;
                hits.push((abs, abs + phrase_bytes, entry.message));
                search_from = abs + phrase_bytes;
            }
        }

        if hits.is_empty() {
            return;
        }

        // Deduplicate: if a shorter phrase starts at the same position as a longer phrase, drop
        // the shorter one. Sort by start asc, then by length desc so that for equal starts the
        // longest hit appears first.
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

        let mut last_end = 0usize;
        for (start, end, message) in hits {
            if start < last_end {
                // Overlapping with a previously emitted (longer) hit — skip.
                continue;
            }
            let abs_start = base.saturating_add(start as u32);
            let abs_end = base.saturating_add(end as u32);
            cx.report(Span::new(abs_start, abs_end), message);
            last_end = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_kamoshiremasen() {
        // 「かもしれ」covers both ない and ません forms.
        let diags = diagnose(&JaNoWeakPhrase::new(), "問題があるかもしれません。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("かもしれ"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_kamo_with_kuten() {
        let diags = diagnose(&JaNoWeakPhrase::new(), "正しいかも。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("かも"), "{}", diags[0].message);
    }

    #[test]
    fn flags_omou_base() {
        let diags = diagnose(&JaNoWeakPhrase::new(), "良いと思う。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("思う"), "{}", diags[0].message);
    }

    #[test]
    fn flags_omoimasu_not_double_reported() {
        // 「思います」does NOT contain 「思う」as a substring (思う = 思 + う, 思います = 思 + い + ま + す),
        // so only the 「思います」entry matches here — exactly one diagnostic. (The longer-match
        // dedup is not what limits the count in this case.)
        let diags = diagnose(&JaNoWeakPhrase::new(), "良いと思います。\n");
        // Expect exactly one diagnostic (for 思います).
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("思います"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_kanou_shisa() {
        let diags = diagnose(&JaNoWeakPhrase::new(), "バグがある可能性を示唆している。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("可能性を示唆している"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn multiple_weak_phrases_in_one_node() {
        // 「思う」and「かもしれ」both present → two diagnostics.
        let diags = diagnose(&JaNoWeakPhrase::new(), "良いと思うがかもしれない。\n");
        assert_eq!(diags.len(), 2, "{diags:?}");
    }

    #[test]
    fn clean_text_is_not_flagged() {
        assert!(diagnose(&JaNoWeakPhrase::new(), "この手法は有効です。\n").is_empty());
    }
}
