//! `no-mixed-zenkaku-hankaku-alphabet` — flag mixing half- and full-width Latin letters.
//!
//! This is a document-level rule: it must see the whole document to decide which width is the
//! minority. It registers [`NodeKind::ROOT`] and walks the subtree from its single `check` call
//! (the `Context` has no cross-call scratch and `&self` is immutable), pruning code subtrees and
//! collecting letter spans from text nodes.

use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

use crate::util::is_fullwidth_alpha;

/// The rule id.
pub const ID: &str = "no-mixed-zenkaku-hankaku-alphabet";

/// Flags the minority-width Latin letters when both half- and full-width letters appear.
pub struct NoMixedZenkakuHankakuAlphabet {
    meta: RuleMeta,
}

impl NoMixedZenkakuHankakuAlphabet {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        NoMixedZenkakuHankakuAlphabet {
            meta: RuleMeta::new(ID, Severity::Warning, vec![NodeKind::ROOT]),
        }
    }
}

impl Default for NoMixedZenkakuHankakuAlphabet {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoMixedZenkakuHankakuAlphabet {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, root: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let mut half_hits: Vec<Span> = Vec::new();
        let mut full_hits: Vec<Span> = Vec::new();

        // Visited-guarded DFS, not descending into code (its literal is node text, not child
        // TEXT). The bitmap bounds work to the node count and makes a byte-valid but cyclic /
        // shared-child archive (`access` validates bytes, not the link graph) neither loop nor
        // OOM — mirroring the engine's traversal.
        let mut visited = vec![false; cx.ast().len()];
        if let Some(slot) = visited.get_mut(root.id().0 as usize) {
            *slot = true;
        }
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                NodeKind::CODE | NodeKind::INLINE_CODE => {}
                NodeKind::TEXT => {
                    let base = node.span().start;
                    for (i, c) in node.text().char_indices() {
                        let span = Span::new(
                            base.saturating_add(i as u32),
                            base.saturating_add((i + c.len_utf8()) as u32),
                        );
                        if c.is_ascii_alphabetic() {
                            half_hits.push(span);
                        } else if is_fullwidth_alpha(c) {
                            full_hits.push(span);
                        }
                    }
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

        // Only a mix of both widths is a violation.
        if half_hits.is_empty() || full_hits.is_empty() {
            return;
        }
        // Report the minority width; ties favor full-width as the minority.
        let (minority, majority_label) = if full_hits.len() <= half_hits.len() {
            (&full_hits, "半角英字")
        } else {
            (&half_hits, "全角英字")
        };
        let message = format!(
            "半角英字と全角英字が混在しています。文書全体で多く使われている{majority_label}に統一してください。"
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
    fn flags_minority_width() {
        // Two half-width + one full-width → the full-width 'Ｂ' is the minority → one diagnostic.
        let diags = diagnose(&NoMixedZenkakuHankakuAlphabet::new(), "AＢC\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("半角英字"));
    }

    #[test]
    fn majority_full_width_flags_half_width() {
        // More full-width than half-width → the half-width 'd' is the minority; majority 全角英字.
        let diags = diagnose(&NoMixedZenkakuHankakuAlphabet::new(), "ＡＢＣd\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("全角英字"));
    }

    #[test]
    fn single_width_is_clean() {
        assert!(diagnose(&NoMixedZenkakuHankakuAlphabet::new(), "ABC\n").is_empty());
        assert!(diagnose(&NoMixedZenkakuHankakuAlphabet::new(), "ＡＢＣ\n").is_empty());
    }

    #[test]
    fn code_is_not_counted() {
        // The full-width 'Ａ' lives in inline code, so the prose has only half-width → no mix.
        assert!(diagnose(&NoMixedZenkakuHankakuAlphabet::new(), "`Ａ`ABC\n").is_empty());
    }

    #[test]
    fn cyclic_archive_does_not_hang() {
        // This rule self-walks the tree, so it must be cycle-safe: a byte-valid but cyclic
        // archive (`access` validates bytes, not the link graph) must not loop. Run on a worker
        // thread and require it to finish quickly (a clean failure if the visited guard regresses).
        use std::sync::mpsc;
        use std::time::Duration;

        use tzlint_ast::{Ast, Node, NodeId, OptionNodeId, Span};
        use tzlint_pdk::Context;

        // ROOT(0) → PARAGRAPH(1) whose first_child points back to itself.
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
            let rule = NoMixedZenkakuHankakuAlphabet::new();
            let mut cx = Context::new(
                archived,
                "no-mixed-zenkaku-hankaku-alphabet".into(),
                Severity::Warning,
            );
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
