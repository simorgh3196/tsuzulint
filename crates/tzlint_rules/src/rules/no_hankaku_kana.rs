//! `no-hankaku-kana` — flag half-width katakana (full-width is preferred).

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

use crate::util::is_halfwidth_kana;

/// The rule id.
pub const ID: &str = "no-hankaku-kana";
const MESSAGE: &str = "半角カタカナは推奨されません。全角カタカナを使ってください。";

/// Flags any run of half-width katakana characters.
pub struct NoHankakuKana {
    meta: RuleMeta,
}

impl NoHankakuKana {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoHankakuKana {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::TEXT]).for_language(Lang::JA),
        }
    }
}

impl Default for NoHankakuKana {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoHankakuKana {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();
        let mut run_start: Option<usize> = None; // byte offset within the node
        for (i, c) in text.char_indices() {
            if is_halfwidth_kana(c) {
                if run_start.is_none() {
                    run_start = Some(i);
                }
            } else if let Some(start) = run_start.take() {
                cx.report(
                    Span::new(
                        base.saturating_add(start as u32),
                        base.saturating_add(i as u32),
                    ),
                    MESSAGE,
                );
            }
        }
        if let Some(start) = run_start {
            cx.report(
                Span::new(
                    base.saturating_add(start as u32),
                    base.saturating_add(text.len() as u32),
                ),
                MESSAGE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_each_run_once() {
        // A single consecutive run is one diagnostic.
        assert_eq!(diagnose(&NoHankakuKana::new(), "ｺﾝﾆﾁﾊ\n").len(), 1);
        // Two runs separated by full-width text → two diagnostics.
        assert_eq!(diagnose(&NoHankakuKana::new(), "ｱあｲ\n").len(), 2);
    }

    #[test]
    fn full_width_kana_is_clean() {
        assert!(diagnose(&NoHankakuKana::new(), "コンニチハ\n").is_empty());
    }
}
