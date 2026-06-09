//! `ja-prh` — terminology / 表記ゆれ checking with autofix, the tzlint counterpart of `prh`.
//!
//! Each configured **term** pairs an `expected` spelling with the `patterns` (disallowed
//! spellings) that should become it: e.g. `{ expected: "JavaScript", patterns: ["Javascript"] }`
//! or `{ expected: "サーバー", patterns: ["サーバ"] }`. Every pattern occurrence in prose text is
//! reported with a [`Fix`] that rewrites it to the expected spelling. A match that is **already**
//! the expected form (e.g. the `サーバ` inside an existing `サーバー`) is left alone, so the rule is
//! idempotent and never doubles a suffix.
//!
//! 0.1.0 takes the term list **inline** from config `options.terms`; loading external `prh` YAML
//! dictionaries (the `.prh.yml` format) is a follow-up that will parse those files and feed the
//! same term list. Matching is a literal, case-sensitive substring scan — patterns carry their own
//! precision. It is a surface rule (no morphology) and JA-scoped (R5).

use serde_json::Value;
use tzlint_ast::NodeKind;
use tzlint_ast::morphology::Lang;
use tzlint_pdk::{Context, Fix, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-prh";

/// One terminology entry: the preferred spelling and the spellings that should become it.
struct Term {
    expected: String,
    patterns: Vec<String>,
}

/// Flags configured 表記ゆれ / terminology patterns and rewrites them to the expected spelling.
pub struct JaPrh {
    meta: RuleMeta,
    terms: Vec<Term>,
}

impl JaPrh {
    /// Construct with no terms (a no-op until `options.terms` supplies some).
    pub fn new() -> Self {
        JaPrh {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]).for_language(Lang::JA),
            terms: Vec::new(),
        }
    }

    /// Construct from config `options`: `terms` is an array of `{ expected, pattern?, patterns? }`,
    /// where `expected` is the preferred spelling and `pattern` (string) / `patterns` (array) list
    /// the spellings to rewrite. Entries without a string `expected` are skipped (leniently).
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        let Some(entries) = options.get("terms").and_then(Value::as_array) else {
            return rule;
        };
        for entry in entries {
            let Some(expected) = entry.get("expected").and_then(Value::as_str) else {
                continue;
            };
            let mut patterns: Vec<String> = Vec::new();
            if let Some(single) = entry.get("pattern").and_then(Value::as_str) {
                patterns.push(single.to_string());
            }
            if let Some(many) = entry.get("patterns").and_then(Value::as_array) {
                patterns.extend(many.iter().filter_map(Value::as_str).map(str::to_string));
            }
            rule.terms.push(Term {
                expected: expected.to_string(),
                patterns,
            });
        }
        rule
    }
}

impl Default for JaPrh {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaPrh {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();

        for term in &self.terms {
            for pattern in &term.patterns {
                // A pattern equal to (or contained inside) the expected form would loop; skip the
                // degenerate equal case and let the "already expected" check below handle the rest.
                if pattern.is_empty() || *pattern == term.expected {
                    continue;
                }
                let mut from = 0usize;
                while let Some(rel) = text
                    .get(from..)
                    .and_then(|rest| rest.find(pattern.as_str()))
                {
                    let off = from + rel;
                    from = off + pattern.len();
                    // Skip a match that is already the start of the expected spelling (e.g. the
                    // サーバ inside サーバー) — fixing it would re-introduce the very pattern.
                    if text
                        .get(off..)
                        .is_some_and(|rest| rest.starts_with(term.expected.as_str()))
                    {
                        continue;
                    }
                    let span = tzlint_ast::Span::new(
                        base.saturating_add(off as u32),
                        base.saturating_add(from as u32),
                    );
                    cx.report_with_fixes(
                        span,
                        message(pattern, &term.expected),
                        [Fix::replace(span, term.expected.clone())],
                    );
                }
            }
        }
    }
}

/// The Japanese diagnostic for a 表記ゆれ hit.
fn message(pattern: &str, expected: &str) -> String {
    format!("表記ゆれ: 「{pattern}」は「{expected}」に統一してください。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_rule;
    use crate::test_support::diagnose;
    use serde_json::json;

    fn rule_with(terms: Value) -> JaPrh {
        JaPrh::from_options(&json!({ "terms": terms }))
    }

    #[test]
    fn rewrites_a_disallowed_spelling() {
        let rule = rule_with(json!([{ "expected": "JavaScript", "patterns": ["Javascript"] }]));
        let src = "I love Javascript.\n";
        let diags = diagnose(&rule, src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        let d = &diags[0];
        assert_eq!(
            &src[d.span.start as usize..d.span.end as usize],
            "Javascript"
        );
        assert_eq!(d.fixes.len(), 1);
        assert_eq!(d.fixes[0].replacement, "JavaScript");
        assert!(d.message.contains("JavaScript"), "{}", d.message);
    }

    #[test]
    fn the_expected_spelling_is_not_flagged() {
        // The text already uses the expected spelling → no diagnostic.
        let rule = rule_with(json!([{ "expected": "JavaScript", "patterns": ["Javascript"] }]));
        assert!(diagnose(&rule, "I love JavaScript.\n").is_empty());
    }

    #[test]
    fn a_pattern_that_is_a_prefix_of_expected_is_idempotent() {
        // サーバ → サーバー, but the サーバ inside an existing サーバー must NOT be flagged (no doubling).
        let rule = rule_with(json!([{ "expected": "サーバー", "patterns": ["サーバ"] }]));
        assert!(
            diagnose(&rule, "このサーバーは速い。\n").is_empty(),
            "already expected"
        );
        // A bare サーバ (no ー) IS flagged and fixed to サーバー.
        let diags = diagnose(&rule, "このサーバが遅い。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].fixes[0].replacement, "サーバー");
    }

    #[test]
    fn multiple_occurrences_each_flag() {
        let rule = rule_with(json!([{ "expected": "JavaScript", "patterns": ["Javascript"] }]));
        let diags = diagnose(&rule, "Javascript and Javascript.\n");
        assert_eq!(diags.len(), 2, "{diags:?}");
    }

    #[test]
    fn a_single_pattern_string_is_accepted() {
        // The `pattern` (singular string) form works alongside `patterns`.
        let rule = JaPrh::from_options(&json!({
            "terms": [{ "expected": "全角", "pattern": "ぜんかく" }]
        }));
        let diags = diagnose(&rule, "これはぜんかくです。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].fixes[0].replacement, "全角");
    }

    #[test]
    fn no_terms_configured_is_a_no_op() {
        assert!(diagnose(&JaPrh::new(), "Javascript everywhere.\n").is_empty());
    }

    #[test]
    fn build_rule_routes_the_terms_option() {
        let rule = build_rule(
            ID,
            &json!({ "terms": [{ "expected": "JavaScript", "patterns": ["Javascript"] }] }),
            None,
        )
        .unwrap();
        assert_eq!(diagnose(rule.as_ref(), "use Javascript\n").len(), 1);
    }

    #[test]
    fn patterns_inside_code_spans_are_left_alone() {
        // Inline code is a separate node kind (not TEXT), so a pattern inside `code` is not touched.
        let rule = rule_with(json!([{ "expected": "JavaScript", "patterns": ["Javascript"] }]));
        assert!(diagnose(&rule, "use `Javascript` here\n").is_empty());
    }
}
