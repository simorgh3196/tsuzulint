//! `ja-no-mixed-period` — flag mixing Japanese `。` and ASCII `.` as sentence terminators.
//!
//! Document-level: it registers [`NodeKind::ROOT`] and walks the subtree from its single
//! `check` call (visited-guarded, so a byte-valid but cyclic archive can neither loop nor OOM),
//! collecting `。` and qualifying `.` terminators, then reports the minority style.

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-no-mixed-period";

/// Flags the minority period style when both `。` and `.` terminators appear.
pub struct JaNoMixedPeriod {
    meta: RuleMeta,
}

impl JaNoMixedPeriod {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        JaNoMixedPeriod {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::ROOT]).for_language(Lang::JA),
        }
    }

    /// Collect `。` (Japanese) and qualifying `.` (English) terminator spans from one text node.
    fn collect(base: u32, text: &str, ja_hits: &mut Vec<Span>, en_hits: &mut Vec<Span>) {
        let mut byte_pos = 0usize;
        let mut prev: Option<char> = None;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let len = c.len_utf8();
            let span = Span::new(
                base.saturating_add(byte_pos as u32),
                base.saturating_add((byte_pos + len) as u32),
            );
            match c {
                '。' => ja_hits.push(span),
                '.' => {
                    // A `.` counts as a sentence end only when followed by whitespace / end of
                    // text, and not between digits (a decimal like "1.5").
                    let after = chars.peek().copied();
                    let looks_like_sentence_end = after.is_none_or(char::is_whitespace);
                    let looks_numeric = prev.is_some_and(|p| p.is_ascii_digit())
                        && after.is_some_and(|a| a.is_ascii_digit());
                    if looks_like_sentence_end && !looks_numeric {
                        en_hits.push(span);
                    }
                }
                _ => {}
            }
            byte_pos += len;
            prev = Some(c);
        }
    }
}

impl Default for JaNoMixedPeriod {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaNoMixedPeriod {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, root: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let mut ja_hits: Vec<Span> = Vec::new();
        let mut en_hits: Vec<Span> = Vec::new();

        let mut visited = vec![false; cx.ast().len()];
        if let Some(slot) = visited.get_mut(root.id().0 as usize) {
            *slot = true;
        }
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                NodeKind::CODE | NodeKind::INLINE_CODE => {}
                NodeKind::TEXT => {
                    Self::collect(node.span().start, node.text(), &mut ja_hits, &mut en_hits);
                }
                _ => {
                    for child in node.children() {
                        if let Some(slot) = visited.get_mut(child.id().0 as usize)
                            && !*slot
                        {
                            *slot = true;
                            stack.push(child);
                        }
                    }
                }
            }
        }

        // Only a mix of both styles is a violation.
        if ja_hits.is_empty() || en_hits.is_empty() {
            return;
        }
        // Report the less-frequent style; on a tie, flag the ASCII `.`.
        let (minority, minority_label, majority_label) = if ja_hits.len() < en_hits.len() {
            (&ja_hits, "。", ".")
        } else {
            (&en_hits, ".", "。")
        };
        let message = format!(
            "句点の表記が混在しています。文書全体で多く使われている「{majority_label}」に合わせて「{minority_label}」を書き換えてください。"
        );
        for span in minority {
            cx.report(*span, message.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_minority_period_style() {
        // Two `。` and one `.` → the ASCII `.` is the minority → one diagnostic.
        let diags = diagnose(&JaNoMixedPeriod::new(), "これは一文目。次の文。最後の文.\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("。"));
    }

    #[test]
    fn flags_japanese_period_when_it_is_the_minority() {
        // Two ASCII `.` and one `。` → the `。` is the minority → reported.
        let diags = diagnose(&JaNoMixedPeriod::new(), "First. Second. 日本語。\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("."));
    }

    #[test]
    fn single_style_is_clean() {
        assert!(diagnose(&JaNoMixedPeriod::new(), "一文目。二文目。\n").is_empty());
        assert!(diagnose(&JaNoMixedPeriod::new(), "First. Second.\n").is_empty());
    }

    #[test]
    fn periods_inside_code_are_not_counted() {
        // The `.` inside inline code is pruned (CODE/INLINE_CODE arm); only the prose `。` and
        // `.` are counted, so this still flags the minority.
        let diags = diagnose(&JaNoMixedPeriod::new(), "`x.y` の結果。Then done.\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn decimal_dot_is_not_a_terminator() {
        // The `.` in "3.14" is between digits → not counted, so no mix with `。`.
        assert!(diagnose(&JaNoMixedPeriod::new(), "円周率は 3.14 です。\n").is_empty());
    }

    #[test]
    fn cyclic_archive_does_not_hang() {
        // Like no-mixed, this rule self-walks the tree, so it must be cycle-safe.
        use std::sync::mpsc;
        use std::time::Duration;

        use tzlint_ast::{Ast, Node, NodeId, OptionNodeId};
        use tzlint_pdk::Context;

        let ast = Ast {
            nodes: vec![
                Node {
                    kind: NodeKind::ROOT,
                    span: Span::new(0, 1),
                    parent: NodeId(0),
                    first_child: OptionNodeId::some(NodeId(1)),
                    next_sibling: OptionNodeId::NONE,
                },
                Node {
                    kind: NodeKind::PARAGRAPH,
                    span: Span::new(0, 1),
                    parent: NodeId(0),
                    first_child: OptionNodeId::some(NodeId(1)), // self-cycle
                    next_sibling: OptionNodeId::NONE,
                },
            ],
            text: "x".to_string(),
            root: NodeId(0),
        };
        let bytes = tzlint_ast::to_archive(&ast).unwrap();

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let archived = tzlint_ast::access(&bytes).unwrap();
            let rule = JaNoMixedPeriod::new();
            let mut cx = Context::new(archived, "ja-no-mixed-period".into(), Severity::Warning);
            if let Some(root) = NodeRef::root(archived) {
                rule.check(root, &mut cx);
            }
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the rule hung on a cyclic archive (visited guard regressed?)"
        );
    }
}
