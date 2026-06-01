//! `no-exclamation-question-mark` — flag `!`/`?` (half- and full-width) in technical prose.

use serde_json::Value;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-exclamation-question-mark";

/// Flags exclamation/question marks; each of the four marks can be individually allowed.
pub struct NoExclamationQuestionMark {
    meta: RuleMeta,
    allow_halfwidth_exclamation: bool,
    allow_fullwidth_exclamation: bool,
    allow_halfwidth_question: bool,
    allow_fullwidth_question: bool,
}

impl NoExclamationQuestionMark {
    /// Construct with the default options (all four marks flagged).
    pub fn new() -> Self {
        NoExclamationQuestionMark {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]),
            allow_halfwidth_exclamation: false,
            allow_fullwidth_exclamation: false,
            allow_halfwidth_question: false,
            allow_fullwidth_question: false,
        }
    }

    /// Construct from config `options`, leniently. Each `allow_*` boolean defaults to `false`.
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        let get = |key: &str| options.get(key).and_then(Value::as_bool);
        if let Some(b) = get("allow_halfwidth_exclamation") {
            rule.allow_halfwidth_exclamation = b;
        }
        if let Some(b) = get("allow_fullwidth_exclamation") {
            rule.allow_fullwidth_exclamation = b;
        }
        if let Some(b) = get("allow_halfwidth_question") {
            rule.allow_halfwidth_question = b;
        }
        if let Some(b) = get("allow_fullwidth_question") {
            rule.allow_fullwidth_question = b;
        }
        rule
    }

    /// The mark to flag for `c`, or `None` if `c` is not a (disallowed) mark.
    fn flagged_mark(&self, c: char) -> Option<&'static str> {
        match c {
            '!' if !self.allow_halfwidth_exclamation => Some("!"),
            '！' if !self.allow_fullwidth_exclamation => Some("！"),
            '?' if !self.allow_halfwidth_question => Some("?"),
            '？' if !self.allow_fullwidth_question => Some("？"),
            _ => None,
        }
    }
}

impl Default for NoExclamationQuestionMark {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoExclamationQuestionMark {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        for (i, c) in node.text().char_indices() {
            if let Some(mark) = self.flagged_mark(c) {
                cx.report(
                    Span::new(
                        base.saturating_add(i as u32),
                        base.saturating_add((i + c.len_utf8()) as u32),
                    ),
                    format!("「{mark}」は技術文書では使用を避けてください。"),
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
    fn flags_all_marks_by_default() {
        let diags = diagnose(&NoExclamationQuestionMark::new(), "すごい！ほんと?\n");
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn allow_options_suppress_specific_marks() {
        let rule = NoExclamationQuestionMark::from_options(
            &serde_json::json!({"allow_fullwidth_exclamation": true}),
        );
        // The full-width ！ is allowed; the half-width ? is still flagged.
        assert_eq!(diagnose(&rule, "やった！でも本当に?\n").len(), 1);
    }

    #[test]
    fn flags_each_of_the_four_marks() {
        // Half/full-width exclamation and question are all flagged by default.
        assert_eq!(
            diagnose(&NoExclamationQuestionMark::new(), "!！?？\n").len(),
            4
        );
    }

    #[test]
    fn each_mark_can_be_individually_allowed() {
        let rule = NoExclamationQuestionMark::from_options(&serde_json::json!({
            "allow_halfwidth_exclamation": true,
            "allow_fullwidth_exclamation": true,
            "allow_halfwidth_question": true,
            "allow_fullwidth_question": true,
        }));
        assert!(diagnose(&rule, "!！?？\n").is_empty());
    }

    #[test]
    fn plain_text_is_clean() {
        assert!(diagnose(&NoExclamationQuestionMark::new(), "ふつうの文。\n").is_empty());
    }
}
