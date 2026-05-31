//! `max-kanji-continuous-len` — flag runs of consecutive kanji longer than a limit.

use serde_json::Value;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

use crate::util::is_kanji;

/// The rule id.
pub const ID: &str = "max-kanji-continuous-len";
/// Default maximum consecutive-kanji run length, in characters.
const DEFAULT_MAX: usize = 5;

/// Flags any run of consecutive kanji whose length (in characters) exceeds `max`.
pub struct MaxKanjiContinuousLen {
    meta: RuleMeta,
    max: usize,
}

impl MaxKanjiContinuousLen {
    /// Construct with the default options (`max` 5).
    pub fn new() -> Self {
        MaxKanjiContinuousLen {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]),
            max: DEFAULT_MAX,
        }
    }

    /// Construct from config `options`, leniently (reads `max`; missing/wrong-typed keeps default).
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(max) = options.get("max").and_then(Value::as_u64) {
            // Fail safe toward "no limit" on a 32-bit target rather than truncating.
            rule.max = usize::try_from(max).unwrap_or(usize::MAX);
        }
        rule
    }

    fn message(&self, run_len: usize) -> String {
        format!(
            "Kanji run of length {run_len} exceeds the maximum of {}.",
            self.max
        )
    }
}

impl Default for MaxKanjiContinuousLen {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for MaxKanjiContinuousLen {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();
        let mut run_start: Option<usize> = None; // byte offset within the node
        let mut run_len = 0usize; // character count of the current run
        for (i, c) in text.char_indices() {
            if is_kanji(c) {
                if run_start.is_none() {
                    run_start = Some(i);
                }
                run_len += 1;
            } else if let Some(start) = run_start.take() {
                if run_len > self.max {
                    // Span covers the kanji bytes only, excluding the terminating char.
                    cx.report(
                        Span::new(
                            base.saturating_add(start as u32),
                            base.saturating_add(i as u32),
                        ),
                        self.message(run_len),
                    );
                }
                run_len = 0;
            }
        }
        // Flush a run that reaches the end of the text.
        if let Some(start) = run_start
            && run_len > self.max
        {
            cx.report(
                Span::new(
                    base.saturating_add(start as u32),
                    base.saturating_add(text.len() as u32),
                ),
                self.message(run_len),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_run_over_limit() {
        let rule = MaxKanjiContinuousLen::from_options(&serde_json::json!({"max": 6}));
        // 7 consecutive kanji that run to end-of-text → one diagnostic (the trailing flush).
        assert_eq!(diagnose(&rule, "一二三四五六七\n").len(), 1);
        // 7 kanji closed by a non-kanji mid-text → one diagnostic (the in-loop close path).
        assert_eq!(diagnose(&rule, "一二三四五六七です\n").len(), 1);
    }

    #[test]
    fn exactly_max_and_split_runs_pass() {
        let rule = MaxKanjiContinuousLen::from_options(&serde_json::json!({"max": 6}));
        // Exactly 6 passes (strict >).
        assert!(diagnose(&rule, "一二三四五六\n").is_empty());
        // Two short runs broken by a non-kanji each stay under the default max (5).
        assert!(diagnose(&MaxKanjiContinuousLen::new(), "一二三の四五六\n").is_empty());
    }

    #[test]
    fn iteration_mark_breaks_a_run() {
        // 々 (U+3005) is intentionally not a kanji here, so it splits the run.
        let rule = MaxKanjiContinuousLen::from_options(&serde_json::json!({"max": 2}));
        assert!(diagnose(&rule, "人々人\n").is_empty());
    }
}
