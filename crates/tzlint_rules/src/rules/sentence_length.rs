//! `sentence-length` — flag sentences longer than a character limit.

use serde_json::Value;
use tzlint_ast::NodeKind;
use tzlint_ast::morphology::Lang;
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

use crate::util::{split_sentences, strip_urls};

/// The rule id.
pub const ID: &str = "sentence-length";
/// Default maximum sentence length, in characters.
const DEFAULT_MAX: usize = 100;

/// Flags any sentence (in a prose block) whose character count exceeds `max`.
pub struct SentenceLength {
    meta: RuleMeta,
    max: usize,
    skip_urls: bool,
}

impl SentenceLength {
    /// Construct with the default options (`max` 100, URLs collapsed before counting).
    pub fn new() -> Self {
        SentenceLength {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
            max: DEFAULT_MAX,
            skip_urls: true,
        }
    }

    /// Construct from config `options`, leniently: a missing or wrong-typed value keeps the
    /// default. Reads `max` (integer) and `skip_urls` (bool).
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(max) = options.get("max").and_then(Value::as_u64) {
            // Fail safe toward "no limit" on a 32-bit target rather than truncating.
            rule.max = usize::try_from(max).unwrap_or(usize::MAX);
        }
        if let Some(skip) = options.get("skip_urls").and_then(Value::as_bool) {
            rule.skip_urls = skip;
        }
        rule
    }
}

impl Default for SentenceLength {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for SentenceLength {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let text = node.text();
        // Collapse URLs first (unless disabled) so they neither inflate the length nor split
        // sentences on the dots inside them.
        let stripped;
        let working: &str = if self.skip_urls {
            stripped = strip_urls(text);
            &stripped
        } else {
            text
        };
        for sentence in split_sentences(working) {
            let char_count = sentence.chars().count();
            if char_count > self.max {
                // The span is the whole block (reconciling per-sentence offsets against the
                // URL-stripped working string is not worth the reader benefit).
                cx.report(
                    node.span(),
                    format!(
                        "Sentence is too long ({char_count} characters). Maximum allowed is {}.",
                        self.max
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

    #[test]
    fn flags_over_limit_sentence() {
        let rule = SentenceLength {
            max: 10,
            skip_urls: true,
            ..SentenceLength::new()
        };
        let diags = diagnose(&rule, "これはとても長い一文なので警告されます。\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("too long"));
    }

    #[test]
    fn exactly_max_passes() {
        let rule = SentenceLength {
            max: 5,
            skip_urls: true,
            ..SentenceLength::new()
        };
        // "12345." → sentence "12345." is 6 chars incl '.', so use 5 letters + no delimiter = 5.
        assert!(diagnose(&rule, "12345").is_empty());
    }

    #[test]
    fn urls_collapse_to_one_char() {
        let input = "see https://example.com/very/long/path now";
        // The long URL counts as a single '・', so the sentence stays under the limit.
        let lenient = SentenceLength {
            max: 12,
            skip_urls: true,
            ..SentenceLength::new()
        };
        assert!(diagnose(&lenient, input).is_empty());
        // With skip_urls off the URL counts in full (and its dots even split sentences), so the
        // limit is exceeded — at least one diagnostic.
        let strict = SentenceLength {
            max: 12,
            skip_urls: false,
            ..SentenceLength::new()
        };
        assert!(!diagnose(&strict, input).is_empty());
    }

    #[test]
    fn from_options_is_lenient() {
        assert_eq!(
            SentenceLength::from_options(&serde_json::json!({"max": 5})).max,
            5
        );
        assert!(!SentenceLength::from_options(&serde_json::json!({"skip_urls": false})).skip_urls);
        // Wrong types / missing keys keep defaults.
        assert_eq!(
            SentenceLength::from_options(&serde_json::json!({"max": "x"})).max,
            DEFAULT_MAX
        );
        assert_eq!(SentenceLength::from_options(&Value::Null).max, DEFAULT_MAX);
    }
}
