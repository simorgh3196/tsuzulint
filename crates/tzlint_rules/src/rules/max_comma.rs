//! `max-comma` — flag sentences with too many ASCII commas.

use serde_json::Value;
use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "max-comma";
/// Default maximum number of commas per sentence (upstream default is 4).
const DEFAULT_MAX: usize = 4;
const COMMA: char = ',';
const KUTEN: char = '。';

/// Flags any sentence whose count of ASCII commas (`,`) exceeds `max`.
pub struct MaxComma {
    meta: RuleMeta,
    max: usize,
}

impl MaxComma {
    /// Construct with default options (`max` 4).
    pub fn new() -> Self {
        MaxComma {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
            max: DEFAULT_MAX,
        }
    }

    /// Construct from config `options`, leniently. Reads `max` (integer); missing/wrong-typed
    /// values keep the default.
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(max) = options.get("max").and_then(Value::as_u64) {
            // Fail safe toward "no limit" on a 32-bit target rather than truncating.
            rule.max = usize::try_from(max).unwrap_or(usize::MAX);
        }
        rule
    }

    fn message(&self, count: usize) -> String {
        format!(
            "一文に「{COMMA}」が {count} 個あります。上限は {} 個です。",
            self.max
        )
    }
}

impl Default for MaxComma {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for MaxComma {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();
        let mut comma_count = 0usize;
        let mut sentence_start = 0usize; // byte offset within the block
        for (i, c) in text.char_indices() {
            if c == COMMA {
                comma_count += 1;
            } else if c == KUTEN {
                if comma_count > self.max {
                    let end = i + c.len_utf8(); // include the terminating 。
                    cx.report(
                        Span::new(
                            base.saturating_add(sentence_start as u32),
                            base.saturating_add(end as u32),
                        ),
                        self.message(comma_count),
                    );
                }
                comma_count = 0;
                sentence_start = i + c.len_utf8();
            }
        }
        // A trailing sentence with no terminating 。.
        if comma_count > self.max && sentence_start < text.len() {
            cx.report(
                Span::new(
                    base.saturating_add(sentence_start as u32),
                    base.saturating_add(text.len() as u32),
                ),
                self.message(comma_count),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_too_many_commas() {
        let rule = MaxComma::new(); // max 4
        // 5 commas → exceeds max of 4
        let diags = diagnose(&rule, "a,b,c,d,e,f。\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("上限は 4 個"));
    }

    #[test]
    fn within_limit_passes() {
        let rule = MaxComma::new();
        // Exactly max (4) passes (strict >).
        assert!(diagnose(&rule, "a,b,c,d,e。\n").is_empty());
        // Fewer than max also passes.
        assert!(diagnose(&rule, "a,b,c。\n").is_empty());
    }

    #[test]
    fn trailing_sentence_without_kuten_is_checked() {
        // 5 commas, no terminating 。
        let diags = diagnose(&MaxComma::new(), "a,b,c,d,e,f\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn multiple_sentences_each_checked_independently() {
        let rule = MaxComma::new();
        // First sentence: 5 commas → flagged; second sentence: 2 commas → ok.
        let diags = diagnose(&rule, "a,b,c,d,e,f。g,h,i。\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn from_options_overrides_max() {
        let rule = MaxComma::from_options(&serde_json::json!({"max": 2}));
        // max 2: a sentence with 3 commas exceeds it (strict >).
        assert_eq!(diagnose(&rule, "a,b,c,d。\n").len(), 1);
        assert!(diagnose(&rule, "a,b,c。\n").is_empty());
    }
}
