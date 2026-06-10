//! `ja-prh` — terminology / 表記ゆれ checking with autofix, the tzlint counterpart of `prh`.
//!
//! Each configured **term** pairs an `expected` spelling with the disallowed spellings that should
//! become it. A spelling is either a **literal** (`pattern` / `patterns`, matched verbatim,
//! case-sensitively) or a **regex** (`regexPatterns`, the prh `/source/flags` form). `expected`
//! doubles as the replacement: for a regex match it is a *template* whose `$1`-style references
//! expand to the pattern's capture groups. Every occurrence in prose text is reported with a
//! [`Fix`] that rewrites it. A literal match already at the start of the expected form (the `サーバ`
//! inside an existing `サーバー`), and a regex match whose replacement equals the matched text, are
//! left alone — so the rule is idempotent and never doubles a suffix.
//!
//! Terms come **inline** from config `options.terms` (`{ expected, pattern?, patterns?,
//! regexPatterns? }`); the CLI also feeds terms parsed from external `prh` `.prh.yml` dictionaries
//! (see `tzlint_core::prh`). Regex matching uses `regex-lite` — linear-time (no ReDoS) and
//! non-panicking; a pattern it cannot compile (e.g. lookaround) is skipped at construction. It is a
//! surface rule (no morphology) and JA-scoped (R5).

use regex_lite::{Captures, Regex};
use serde_json::Value;
use tzlint_ast::NodeKind;
use tzlint_ast::morphology::Lang;
use tzlint_pdk::{Context, Fix, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-prh";

/// One terminology entry: the preferred spelling and the spellings that should become it.
struct Term {
    /// The preferred spelling. For a regex match it is a replacement *template* (may carry
    /// `$1`-style capture references); for a literal match it is used verbatim.
    expected: String,
    /// Literal disallowed spellings, matched verbatim and case-sensitively.
    literals: Vec<String>,
    /// Regex disallowed spellings (prh `/source/flags`), compiled once at construction.
    regexes: Vec<Regex>,
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

    /// Construct from config `options`: `terms` is an array of
    /// `{ expected, pattern?, patterns?, regexPatterns? }`, where `expected` is the preferred
    /// spelling, `pattern` (string) / `patterns` (array) list literal spellings, and `regexPatterns`
    /// lists `{ source, ignoreCase?, multiline? }` regular expressions. Entries without a string
    /// `expected` are skipped, and a regex that does not compile is dropped (both leniently).
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        let Some(entries) = options.get("terms").and_then(Value::as_array) else {
            return rule;
        };
        for entry in entries {
            let Some(expected) = entry.get("expected").and_then(Value::as_str) else {
                continue;
            };
            let mut literals: Vec<String> = Vec::new();
            if let Some(single) = entry.get("pattern").and_then(Value::as_str) {
                literals.push(single.to_string());
            }
            if let Some(many) = entry.get("patterns").and_then(Value::as_array) {
                literals.extend(many.iter().filter_map(Value::as_str).map(str::to_string));
            }
            let mut regexes: Vec<Regex> = Vec::new();
            if let Some(items) = entry.get("regexPatterns").and_then(Value::as_array) {
                regexes.extend(items.iter().filter_map(compile_regex));
            }
            rule.terms.push(Term {
                expected: expected.to_string(),
                literals,
                regexes,
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
            // Literal patterns: a verbatim, case-sensitive substring scan.
            for pattern in &term.literals {
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

            // Regex patterns: each match's replacement is the `expected` template with its `$N`
            // capture references expanded. A zero-width match, or one whose replacement already
            // equals the matched text, is left alone (idempotent).
            for regex in &term.regexes {
                for caps in regex.captures_iter(text) {
                    let Some(whole) = caps.get(0) else { continue };
                    if whole.start() == whole.end() {
                        continue;
                    }
                    let replacement = expand_template(&term.expected, &caps);
                    if replacement == whole.as_str() {
                        continue;
                    }
                    let span = tzlint_ast::Span::new(
                        base.saturating_add(whole.start() as u32),
                        base.saturating_add(whole.end() as u32),
                    );
                    cx.report_with_fixes(
                        span,
                        message(whole.as_str(), &replacement),
                        [Fix::replace(span, replacement)],
                    );
                }
            }
        }
    }
}

/// Compile one `regexPatterns` entry (`{ source, ignoreCase?, multiline? }`) into a [`Regex`], or
/// `None` when it has no non-empty string `source` or the source is not a valid `regex-lite`
/// expression (e.g. it uses lookaround/backreferences, which neither `regex-lite` nor `regex`
/// support). Skipping such a pattern keeps the rest of the dictionary working — best-effort prh
/// migration.
fn compile_regex(item: &Value) -> Option<Regex> {
    let source = item.get("source").and_then(Value::as_str)?;
    if source.is_empty() {
        return None;
    }
    let ignore_case = item
        .get("ignoreCase")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let multiline = item
        .get("multiline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // `regex-lite` honors the `(?i)` / `(?m)` inline flag group at the start of a pattern.
    let mut pattern = String::new();
    if ignore_case || multiline {
        pattern.push_str("(?");
        if ignore_case {
            pattern.push('i');
        }
        if multiline {
            pattern.push('m');
        }
        pattern.push(')');
    }
    pattern.push_str(source);
    Regex::new(&pattern).ok()
}

/// Expand a prh replacement template against a regex match: `$0`..`$N` and `${N}` are replaced by
/// the corresponding capture group (an absent group expands to empty), `$$` is a literal `$`, and
/// any other `$`-sequence is kept verbatim.
fn expand_template(template: &str, caps: &Captures) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('{') => {
                chars.next();
                let mut digits = String::new();
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    if let Some(d) = chars.next() {
                        digits.push(d);
                    }
                }
                let closed = chars.peek() == Some(&'}');
                if closed {
                    chars.next();
                }
                match (closed, digits.parse::<usize>()) {
                    (true, Ok(n)) => push_group(&mut out, caps, n),
                    _ => {
                        // Malformed `${…}`: keep the consumed text verbatim.
                        out.push('$');
                        out.push('{');
                        out.push_str(&digits);
                        if closed {
                            out.push('}');
                        }
                    }
                }
            }
            Some(d) if d.is_ascii_digit() => {
                let mut digits = String::new();
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    if let Some(d) = chars.next() {
                        digits.push(d);
                    }
                }
                match digits.parse::<usize>() {
                    Ok(n) => push_group(&mut out, caps, n),
                    Err(_) => {
                        out.push('$');
                        out.push_str(&digits);
                    }
                }
            }
            _ => out.push('$'),
        }
    }
    out
}

/// Append capture group `n` (the whole match for `0`) to `out`, or nothing when it did not match.
fn push_group(out: &mut String, caps: &Captures, n: usize) {
    if let Some(m) = caps.get(n) {
        out.push_str(m.as_str());
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

    #[test]
    fn rewrites_a_regex_pattern_with_a_capture_template() {
        // The canonical prh form: a regex pattern whose `expected` is a `$1` capture template.
        let rule = rule_with(json!([{
            "expected": "（$1）",
            "regexPatterns": [{ "source": "\\(([^)]+)\\)" }]
        }]));
        let diags = diagnose(&rule, "これは(補足)です。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].fixes.len(), 1);
        assert_eq!(diags[0].fixes[0].replacement, "（補足）");
    }

    #[test]
    fn the_i_flag_matches_case_insensitively() {
        let rule = rule_with(json!([{
            "expected": "JavaScript",
            "regexPatterns": [{ "source": "javascript", "ignoreCase": true }]
        }]));
        let diags = diagnose(&rule, "I love JAVASCRIPT today.\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].fixes[0].replacement, "JavaScript");
    }

    #[test]
    fn a_regex_match_already_in_expected_form_is_not_flagged() {
        // The replacement equals the matched text → no-op, so it must not be reported (idempotent).
        let rule = rule_with(json!([{
            "expected": "$1",
            "regexPatterns": [{ "source": "(foo)" }]
        }]));
        assert!(diagnose(&rule, "foo bar\n").is_empty());
    }

    #[test]
    fn an_uncompilable_regex_is_skipped() {
        // regex-lite rejects lookaround; the pattern is dropped at construction (no panic, no diag).
        let rule = rule_with(json!([{
            "expected": "X",
            "regexPatterns": [{ "source": "(?=x)" }]
        }]));
        assert!(diagnose(&rule, "xxx\n").is_empty());
    }

    #[test]
    fn literal_and_regex_patterns_coexist_on_one_term() {
        let rule = rule_with(json!([{
            "expected": "JavaScript",
            "patterns": ["Javascript"],
            "regexPatterns": [{ "source": "java-script", "ignoreCase": true }]
        }]));
        let diags = diagnose(&rule, "use Javascript and JAVA-SCRIPT.\n");
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.fixes[0].replacement == "JavaScript"));
    }

    #[test]
    fn the_i_and_m_flags_compile_together() {
        // Both flags fold into a leading `(?im)` group at compile time; the pattern still rewrites.
        let rule = rule_with(json!([{
            "expected": "JavaScript",
            "regexPatterns": [{ "source": "javascript", "ignoreCase": true, "multiline": true }]
        }]));
        let diags = diagnose(&rule, "use JavaScript or javascript here.\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].fixes[0].replacement, "JavaScript");
    }

    #[test]
    fn a_zero_width_regex_match_is_left_alone() {
        // A pattern that can match the empty string must not emit empty, zero-width rewrites.
        let rule = rule_with(json!([{
            "expected": "x",
            "regexPatterns": [{ "source": "a*" }]
        }]));
        assert!(diagnose(&rule, "bbb\n").is_empty());
    }

    #[test]
    fn expand_template_covers_every_dollar_form() {
        // Two indexed groups: `$0` = "foo-bar", `$1` = "foo", `$2` = "bar".
        let re = Regex::new(r"(\w+)-(\w+)").unwrap();
        let caps = re.captures("foo-bar").unwrap();

        // Bare `$N`, the braced `${N}`, and `$0` (the whole match).
        assert_eq!(expand_template("$1", &caps), "foo");
        assert_eq!(expand_template("${2}", &caps), "bar");
        assert_eq!(expand_template("$0", &caps), "foo-bar");
        assert_eq!(expand_template("[${1}/$2]", &caps), "[foo/bar]");
        // An absent group expands to nothing, in either form.
        assert_eq!(expand_template("a${9}b", &caps), "ab");
        assert_eq!(expand_template("a$9b", &caps), "ab");
        // `$$` is a literal dollar.
        assert_eq!(expand_template("$$1", &caps), "$1");
        // Malformed `${…}` — unterminated, or non-numeric — is kept verbatim.
        assert_eq!(expand_template("${1", &caps), "${1");
        assert_eq!(expand_template("${x}", &caps), "${x}");
        assert_eq!(expand_template("${}", &caps), "${}");
        // A `$` not followed by a digit/brace/`$` is kept verbatim, trailing one included.
        assert_eq!(expand_template("$x", &caps), "$x");
        assert_eq!(expand_template("a$", &caps), "a$");
        // An index that overflows `usize` is kept literally rather than panicking.
        assert_eq!(
            expand_template("$99999999999999999999999999", &caps),
            "$99999999999999999999999999"
        );
    }
}
