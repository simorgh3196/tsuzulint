//! `max-ten` — flag sentences with too many 読点 (Japanese commas).

use serde_json::Value;
use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "max-ten";
/// Default maximum number of 読点 per sentence.
const DEFAULT_MAX: usize = 3;
const DEFAULT_TOUTEN: char = '、';
const DEFAULT_KUTEN: char = '。';

/// Flags any sentence whose count of `touten` (読点) exceeds `max`.
pub struct MaxTen {
    meta: RuleMeta,
    max: usize,
    touten: char,
    kuten: char,
}

impl MaxTen {
    /// Construct with default options (`max` 3, `touten` 「、」, `kuten` 「。」).
    pub fn new() -> Self {
        MaxTen {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
            max: DEFAULT_MAX,
            touten: DEFAULT_TOUTEN,
            kuten: DEFAULT_KUTEN,
        }
    }

    /// Construct from config `options`, leniently. Reads `max` (integer), `touten` and `kuten`
    /// (first character of a string); missing/wrong-typed values keep the defaults.
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(max) = options.get("max").and_then(Value::as_u64) {
            // Fail safe toward "no limit" on a 32-bit target rather than truncating.
            rule.max = usize::try_from(max).unwrap_or(usize::MAX);
        }
        if let Some(c) = options
            .get("touten")
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
        {
            rule.touten = c;
        }
        if let Some(c) = options
            .get("kuten")
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
        {
            rule.kuten = c;
        }
        rule
    }

    fn message(&self, count: usize) -> String {
        format!(
            "一文に「{}」が {count} 個あります。上限は {} 個です。",
            self.touten, self.max
        )
    }
}

impl Default for MaxTen {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for MaxTen {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();
        let mut ten_count = 0usize;
        let mut sentence_start = 0usize; // byte offset within the block
        for (i, c) in text.char_indices() {
            if c == self.touten {
                ten_count += 1;
            } else if c == self.kuten {
                if ten_count > self.max {
                    let end = i + c.len_utf8(); // include the terminating 。
                    cx.report(
                        Span::new(
                            base.saturating_add(sentence_start as u32),
                            base.saturating_add(end as u32),
                        ),
                        self.message(ten_count),
                    );
                }
                ten_count = 0;
                sentence_start = i + c.len_utf8();
            }
        }
        // A trailing sentence with no terminating 。.
        if ten_count > self.max && sentence_start < text.len() {
            cx.report(
                Span::new(
                    base.saturating_add(sentence_start as u32),
                    base.saturating_add(text.len() as u32),
                ),
                self.message(ten_count),
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
        let rule = MaxTen::new(); // max 3
        let diags = diagnose(&rule, "あ、い、う、え、お。\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("上限は 3 個"));
    }

    #[test]
    fn within_limit_passes() {
        assert!(diagnose(&MaxTen::new(), "あ、い、う。\n").is_empty());
        // Exactly max passes (strict >).
        assert!(diagnose(&MaxTen::new(), "あ、い、う、え。\n").is_empty());
    }

    #[test]
    fn trailing_sentence_without_kuten_is_checked() {
        let diags = diagnose(&MaxTen::new(), "あ、い、う、え、お\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn from_options_overrides() {
        let rule = MaxTen::from_options(&serde_json::json!({"max": 1}));
        // max 1: a sentence with 2 読点 exceeds it (1 alone would pass — strict >).
        assert_eq!(diagnose(&rule, "あ、い、う。\n").len(), 1);
        assert!(diagnose(&rule, "あ、い。\n").is_empty());
        assert_eq!(
            MaxTen::from_options(&serde_json::json!({"touten": ","})).touten,
            ','
        );
        assert_eq!(
            MaxTen::from_options(&serde_json::json!({"kuten": "."})).kuten,
            '.'
        );
    }
}
