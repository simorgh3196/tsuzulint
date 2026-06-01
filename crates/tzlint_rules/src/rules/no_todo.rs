//! `no-todo` — flag task markers (TODO/FIXME/XXX/HACK) in prose.

use serde_json::Value;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-todo";
/// Default marker patterns (trailing space/colon distinguishes a marker from a word).
const DEFAULT_PATTERNS: &[&str] = &[
    "TODO:", "TODO ", "FIXME:", "FIXME ", "XXX:", "XXX ", "HACK:",
];

/// Flags configured task-marker substrings in prose.
pub struct NoTodo {
    meta: RuleMeta,
    patterns: Vec<String>,
    ignore_patterns: Vec<String>,
    case_sensitive: bool,
}

impl NoTodo {
    /// Construct with the default markers (case-insensitive, no ignore list).
    pub fn new() -> Self {
        NoTodo {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]),
            patterns: DEFAULT_PATTERNS.iter().map(|s| (*s).to_string()).collect(),
            ignore_patterns: Vec::new(),
            case_sensitive: false,
        }
    }

    /// Construct from config `options`, leniently. Reads `patterns` (string[]),
    /// `ignore_patterns` (string[]), and `case_sensitive` (bool). Empty pattern strings are
    /// dropped (an empty pattern would match everywhere); if no usable pattern remains, the
    /// defaults are kept.
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(arr) = options.get("patterns").and_then(Value::as_array) {
            let patterns: Vec<String> = arr
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            if !patterns.is_empty() {
                rule.patterns = patterns;
            }
        }
        if let Some(arr) = options.get("ignore_patterns").and_then(Value::as_array) {
            rule.ignore_patterns = arr
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        if let Some(b) = options.get("case_sensitive").and_then(Value::as_bool) {
            rule.case_sensitive = b;
        }
        rule
    }
}

impl Default for NoTodo {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoTodo {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();
        for pattern in &self.patterns {
            let mut search_start = 0;
            while let Some((start, end)) =
                find_pattern(text, pattern, search_start, self.case_sensitive)
            {
                let matched = &text[start..end];
                let ignored = self
                    .ignore_patterns
                    .iter()
                    .any(|ignore| find_pattern(matched, ignore, 0, self.case_sensitive).is_some());
                if !ignored {
                    let display = pattern.trim_end();
                    cx.report(
                        Span::new(
                            base.saturating_add(start as u32),
                            base.saturating_add(end as u32),
                        ),
                        format!(
                            "Found '{display}' marker. Consider resolving this before committing."
                        ),
                    );
                }
                search_start = end; // `end > start` for a non-empty pattern, so this terminates
            }
        }
    }
}

/// Find the first occurrence of `pattern` in `haystack[start..]`, returning byte offsets into
/// `haystack`. Case-insensitive matching is anchored to the original bytes (never slicing a
/// lowercased copy, whose byte length can differ), so all offsets stay on char boundaries.
/// An empty pattern never matches (returns `None`) so callers cannot loop forever.
fn find_pattern(
    haystack: &str,
    pattern: &str,
    start: usize,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    if pattern.is_empty() || start > haystack.len() {
        return None;
    }
    if case_sensitive {
        let idx = haystack[start..].find(pattern)?;
        let match_start = start + idx;
        return Some((match_start, match_start + pattern.len()));
    }

    let pattern_chars: Vec<char> = pattern.chars().collect();
    for (offset, _) in haystack[start..].char_indices() {
        let anchor = start + offset;
        let mut cursor = anchor;
        let mut matched = true;
        for &pc in &pattern_chars {
            match haystack[cursor..].chars().next() {
                Some(hc) if chars_equal_ignore_case(pc, hc) => cursor += hc.len_utf8(),
                _ => {
                    matched = false;
                    break;
                }
            }
        }
        if matched {
            return Some((anchor, cursor));
        }
    }
    None
}

/// Case-insensitive char equality for single-char comparisons. Characters whose lowercase
/// expands to multiple chars (e.g. `İ`) are treated as non-matching (the markers are ASCII).
fn chars_equal_ignore_case(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    let (mut la, mut lb) = (a.to_lowercase(), b.to_lowercase());
    match (la.next(), lb.next()) {
        (Some(ca), Some(cb)) => ca == cb && la.next().is_none() && lb.next().is_none(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_default_markers_case_insensitively() {
        assert_eq!(diagnose(&NoTodo::new(), "todo: あとで直す\n").len(), 1);
        assert_eq!(diagnose(&NoTodo::new(), "FIXME これ\n").len(), 1);
        // Two markers → two diagnostics.
        assert_eq!(diagnose(&NoTodo::new(), "TODO: a と XXX: b\n").len(), 2);
    }

    #[test]
    fn plain_prose_is_clean() {
        // "todo" without a trailing space/colon is not a default marker.
        assert!(diagnose(&NoTodo::new(), "今日のtodoリスト\n").is_empty());
    }

    #[test]
    fn case_sensitive_and_custom_patterns() {
        let cs = NoTodo::from_options(&serde_json::json!({"case_sensitive": true}));
        assert!(diagnose(&cs, "todo: lower\n").is_empty()); // lowercase not matched
        assert_eq!(diagnose(&cs, "TODO: upper\n").len(), 1);

        let custom = NoTodo::from_options(&serde_json::json!({"patterns": ["WIP"]}));
        assert_eq!(diagnose(&custom, "WIP の節\n").len(), 1);
        assert!(diagnose(&custom, "TODO: not configured\n").is_empty());
    }

    #[test]
    fn ignore_patterns_suppress_matches() {
        // `ignore_patterns` are matched against the matched MARKER text (e.g. "FIXME "), not the
        // surrounding prose — faithful to the legacy rule.
        let rule = NoTodo::from_options(&serde_json::json!({"ignore_patterns": ["FIXME"]}));
        assert!(diagnose(&rule, "FIXME これ\n").is_empty()); // "FIXME " contains "FIXME"
        assert_eq!(diagnose(&rule, "TODO: あれ\n").len(), 1); // "TODO: " does not
    }

    #[test]
    fn find_pattern_guards_empty_and_out_of_bounds() {
        // The early return that prevents an empty-pattern infinite loop and an out-of-range start.
        assert_eq!(find_pattern("abc", "", 0, false), None);
        assert_eq!(find_pattern("abc", "x", 99, true), None);
    }

    #[test]
    fn empty_pattern_is_dropped_not_looped() {
        // An empty pattern must not be kept (it would match everywhere / loop); defaults remain.
        let rule = NoTodo::from_options(&serde_json::json!({"patterns": [""]}));
        assert_eq!(diagnose(&rule, "TODO: x\n").len(), 1); // fell back to defaults
        assert!(diagnose(&rule, "plain\n").is_empty());
    }
}
