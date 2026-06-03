//! Applying autofixes with deterministic conflict resolution.

use tzlint_pdk::{Diagnostic, Fix, Rule};

use crate::Engine;

/// Maximum number of lint→fix passes (ESLint semantics), guaranteeing termination.
pub const MAX_FIX_PASSES: usize = 10;

/// The outcome of one [`apply_fixes`] pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPass {
    /// The text after applying the non-conflicting fixes.
    pub output: String,
    /// How many fixes were applied.
    pub applied: usize,
    /// How many fixes were deferred this pass (overlapping or invalid).
    pub deferred: usize,
}

/// Apply the fixes carried by `diagnostics` to `source` in one left-to-right pass.
///
/// Fixes are ordered by `(span.start` ascending`, span.end` descending`)`, so among fixes
/// starting at the same offset the **longest wins**. Non-overlapping fixes apply; a fix
/// overlapping an already-applied one is **deferred** for a later pass, as is any fix whose
/// span is inverted, out of range, or not on UTF-8 char boundaries. Spans are absolute byte
/// offsets into `source`. Zero-width insertions (`start == end`) at the same offset do not
/// overlap, so they all apply, concatenated in the deterministic sort order.
#[must_use]
pub fn apply_fixes(source: &str, diagnostics: &[Diagnostic]) -> FixPass {
    let mut fixes: Vec<&Fix> = diagnostics.iter().flat_map(|d| d.fixes.iter()).collect();
    // Order: start asc, then end desc (longest-at-a-start wins), then replacement — the last
    // key makes the order a deterministic *total* order, independent of input/rule order.
    fixes.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then(b.span.end.cmp(&a.span.end))
            .then_with(|| a.replacement.cmp(&b.replacement))
    });

    let mut output = String::with_capacity(source.len());
    let mut cursor: usize = 0; // bytes of `source` already copied into `output`
    let mut applied = 0;
    let mut deferred = 0;

    for fix in fixes {
        let start = fix.span.start as usize;
        let end = fix.span.end as usize;
        // Defer inverted/out-of-range spans and any that overlap an applied fix.
        if end < start || end > source.len() || start < cursor {
            deferred += 1;
            continue;
        }
        // Both the gap and the replaced region must slice on char boundaries.
        let (Some(gap), Some(_)) = (source.get(cursor..start), source.get(start..end)) else {
            deferred += 1;
            continue;
        };
        output.push_str(gap);
        output.push_str(&fix.replacement);
        cursor = end;
        applied += 1;
    }
    if let Some(tail) = source.get(cursor..) {
        output.push_str(tail);
    }
    FixPass {
        output,
        applied,
        deferred,
    }
}

/// Lint-and-fix `source` to a fixpoint: parse → lint → apply, repeating until the text
/// stops changing or [`MAX_FIX_PASSES`] is reached (which guarantees termination). Returns
/// the fixed text; a parse failure leaves the current text unchanged.
#[must_use]
pub fn fix(source: &str, rules: &[&dyn Rule]) -> String {
    let mut text = source.to_string();
    for _ in 0..MAX_FIX_PASSES {
        let Ok(ast) = crate::parse(&text) else { break };
        let Ok(bytes) = tzlint_ast::to_archive(&ast) else {
            break;
        };
        let Ok(archived) = tzlint_ast::access(&bytes) else {
            break;
        };
        // No morphology on the fix path yet (wired alongside the cached/CLI paths in M2e/M2h).
        let diagnostics = Engine::lint(archived, None, rules);
        let pass = apply_fixes(&text, &diagnostics);
        if pass.output == text {
            break; // fixpoint: nothing changed
        }
        text = pass.output;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use tzlint_ast::{NodeKind, Span};
    use tzlint_pdk::{Context, NodeRef, RuleMeta, Severity};

    fn diag_with_fix(span: Span, replacement: &str) -> Diagnostic {
        Diagnostic::new("t", Severity::Warning, span, "m").with_fix(Fix::replace(span, replacement))
    }

    #[test]
    fn applies_non_overlapping_fixes_left_to_right() {
        // "0123456789": replace [1,3) with "A" and [5,7) with "B".
        let diags = [
            diag_with_fix(Span::new(1, 3), "A"),
            diag_with_fix(Span::new(5, 7), "B"),
        ];
        let pass = apply_fixes("0123456789", &diags);
        assert_eq!(pass.output, "0A34B789");
        assert_eq!((pass.applied, pass.deferred), (2, 0));
    }

    #[test]
    fn overlapping_fixes_longest_wins_other_deferred() {
        // Same start: [1,5) "X" (longer) beats [1,3) "Y"; the loser is deferred.
        let diags = [
            diag_with_fix(Span::new(1, 3), "Y"),
            diag_with_fix(Span::new(1, 5), "X"),
        ];
        let pass = apply_fixes("012345", &diags);
        assert_eq!(pass.output, "0X5");
        assert_eq!((pass.applied, pass.deferred), (1, 1));
    }

    #[test]
    fn invalid_spans_are_deferred_not_applied() {
        let diags = [
            diag_with_fix(Span::new(3, 2), "inverted"),
            diag_with_fix(Span::new(0, 999), "out of range"),
        ];
        let pass = apply_fixes("hello", &diags);
        assert_eq!(pass.output, "hello"); // unchanged
        assert_eq!((pass.applied, pass.deferred), (0, 2));
    }

    #[test]
    fn zero_width_insertions_at_same_offset_all_apply_in_sort_order() {
        // Two pure insertions at offset 1 (empty spans). Neither overlaps, so both apply;
        // the order is deterministic by replacement ("A" before "B").
        let diags = [
            diag_with_fix(Span::new(1, 1), "B"),
            diag_with_fix(Span::new(1, 1), "A"),
        ];
        let pass = apply_fixes("xy", &diags);
        assert_eq!(pass.output, "xABy");
        assert_eq!((pass.applied, pass.deferred), (2, 0));
    }

    #[test]
    fn fix_splitting_a_multibyte_char_is_deferred() {
        // "あ" is 3 bytes; the span [0,1) ends inside the char (not a UTF-8 boundary), so the
        // fix can't be sliced and is deferred rather than corrupting the output.
        let diags = [diag_with_fix(Span::new(0, 1), "x")];
        let pass = apply_fixes("あ", &diags);
        assert_eq!(pass.output, "あ");
        assert_eq!((pass.applied, pass.deferred), (0, 1));
    }

    /// Replaces each Text node's content with `find`→`replace` (a whole-span swap).
    struct Rewrite {
        meta: RuleMeta,
        find: &'static str,
        replace: &'static str,
    }
    impl Rewrite {
        fn new(find: &'static str, replace: &'static str) -> Self {
            Rewrite {
                meta: RuleMeta::new("rewrite", Severity::Warning, vec![NodeKind::TEXT]),
                find,
                replace,
            }
        }
    }
    impl Rule for Rewrite {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            if node.text() == self.find {
                cx.report_with_fixes(
                    node.span(),
                    "rewrite",
                    [Fix::replace(node.span(), self.replace)],
                );
            }
        }
    }

    #[test]
    fn fix_converges_when_no_more_apply() {
        // Once "BAD" → "ok", re-linting finds nothing, so it stops.
        let rule = Rewrite::new("BAD", "ok");
        assert_eq!(fix("BAD\n", &[&rule]), "ok\n");
    }

    #[test]
    fn fix_terminates_at_the_pass_cap() {
        // A rule that always grows the text never reaches a fixpoint; the cap stops it.
        struct Grow {
            meta: RuleMeta,
        }
        impl Rule for Grow {
            fn meta(&self) -> &RuleMeta {
                &self.meta
            }
            fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
                let grown = format!("{}!", node.text());
                cx.report_with_fixes(node.span(), "grow", [Fix::replace(node.span(), grown)]);
            }
        }
        let rule = Grow {
            meta: RuleMeta::new("grow", Severity::Warning, vec![NodeKind::TEXT]),
        };
        // Starts as "a"; each of the 10 passes appends one '!'.
        let out = fix("a", &[&rule]);
        assert_eq!(out, format!("a{}", "!".repeat(MAX_FIX_PASSES)));
    }
}
