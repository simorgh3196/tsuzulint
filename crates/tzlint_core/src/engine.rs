//! The single-traversal, multi-visitor lint engine.

use tzlint_ast::ArchivedAst;
use tzlint_pdk::{Context, Diagnostic, NodeRef, Rule};

/// The lint engine. Stateless (config/rule-registry land later); the one dispatch
/// entry point for CLI, LSP, native, and (future) plugin rules.
pub struct Engine;

impl Engine {
    /// Lint an archived AST with `rules`, returning diagnostics in the stable total order
    /// `(span.start, span.end, rule_id, message)` — independent of traversal/scheduling.
    ///
    /// Native rules share **one** pre-order traversal: the AST is walked once and, at each
    /// node, only the rules whose [`node_kinds`](tzlint_pdk::RuleMeta) include that node's
    /// kind are invoked. Each rule keeps its own [`Context`] across the walk; every rule's
    /// `finish` runs once afterwards. The walk is iterative, so even a deeply nested tree
    /// cannot overflow the stack here.
    #[must_use]
    pub fn lint(ast: &ArchivedAst, rules: &[&dyn Rule]) -> Vec<Diagnostic> {
        // Hoist metadata once (not per node × rule); one context per rule.
        let metas: Vec<&tzlint_pdk::RuleMeta> = rules.iter().map(|rule| rule.meta()).collect();
        let mut contexts: Vec<Context> = metas
            .iter()
            .map(|meta| Context::new(ast, meta.id.clone(), meta.default_severity))
            .collect();

        // Single pre-order walk. A `visited` bitmap makes it cycle-safe and bounds total
        // work to the node count, so a malformed archive — whose links `access` validates as
        // bytes but not as a graph — can neither loop nor OOM here.
        if let Some(root) = NodeRef::root(ast) {
            let mut visited = vec![false; ast.len()];
            if let Some(slot) = visited.get_mut(root.id().0 as usize) {
                *slot = true;
            }
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                let kind = node.kind();
                for (index, cx) in contexts.iter_mut().enumerate() {
                    if metas[index].visits(kind) {
                        rules[index].check(node, cx);
                    }
                }
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

        // Cross-node finalize, then aggregate and sort into the stable total order.
        for (index, cx) in contexts.iter_mut().enumerate() {
            rules[index].finish(cx);
        }
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        for cx in contexts {
            diagnostics.extend(cx.into_diagnostics());
        }
        diagnostics.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use tzlint_ast::{Ast, NodeId, NodeKind, Span};
    use tzlint_pdk::{RuleMeta, Severity};

    /// Reports `message` at every node of `kind`.
    struct FlagKind {
        meta: RuleMeta,
        message: &'static str,
    }
    impl FlagKind {
        fn new(id: &str, kind: NodeKind, message: &'static str) -> Self {
            FlagKind {
                meta: RuleMeta::new(id, Severity::Warning, vec![kind]),
                message,
            }
        }
    }
    impl Rule for FlagKind {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            cx.report(node.span(), self.message);
        }
    }

    /// A document-level rule: registers for `ROOT` (its `check` is a no-op — the work is
    /// cross-node), then in `finish` walks the whole tree from `cx.ast()` (the
    /// subtree-self-traversal pattern) and reports the count once. State lives in locals,
    /// never in `&self`.
    struct CountNodes {
        meta: RuleMeta,
    }
    impl Rule for CountNodes {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, _node: NodeRef<'ast>, _cx: &mut Context<'ast>) {}
        fn finish<'ast>(&self, cx: &mut Context<'ast>) {
            let mut count = 0u32;
            let mut stack: Vec<NodeRef> = NodeRef::root(cx.ast()).into_iter().collect();
            while let Some(node) = stack.pop() {
                count += 1;
                stack.extend(node.children());
            }
            cx.report(Span::new(0, 0), format!("{count} nodes"));
        }
    }

    fn archive(src: &str) -> tzlint_ast::AlignedVec {
        tzlint_ast::to_archive(&parse(src).unwrap()).unwrap()
    }

    #[test]
    fn dispatches_only_to_registered_kinds() {
        // "# H\n\nbody" → Root, Heading, Text("H"), Paragraph, Text("body").
        let bytes = archive("# H\n\nbody");
        let ast = tzlint_ast::access(&bytes).unwrap();

        let text_rule = FlagKind::new("flag-text", NodeKind::TEXT, "text");
        let heading_rule = FlagKind::new("flag-heading", NodeKind::HEADING, "heading");
        let diags = Engine::lint(ast, &[&text_rule, &heading_rule]);

        // 2 Text nodes + 1 Heading = 3 diagnostics.
        assert_eq!(diags.len(), 3);
        assert_eq!(diags.iter().filter(|d| d.message == "text").count(), 2);
        assert_eq!(diags.iter().filter(|d| d.message == "heading").count(), 1);
    }

    #[test]
    fn output_is_in_stable_sorted_order() {
        let bytes = archive("# H\n\nbody");
        let ast = tzlint_ast::access(&bytes).unwrap();
        // Two rules over the same kind produce diagnostics at the same spans; sorting must
        // be deterministic by (start, end, rule_id, message).
        let a = FlagKind::new("a-rule", NodeKind::TEXT, "msg");
        let b = FlagKind::new("b-rule", NodeKind::TEXT, "msg");
        let diags = Engine::lint(ast, &[&b, &a]); // pass b first on purpose
        let keys: Vec<_> = diags
            .iter()
            .map(|d| (d.span.start, d.rule_id.as_str()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "diagnostics must come out sorted");
    }

    #[test]
    fn finish_emits_a_document_level_diagnostic() {
        let bytes = archive("# H\n\nbody");
        let ast = tzlint_ast::access(&bytes).unwrap();
        let counter = CountNodes {
            meta: RuleMeta::new("count", Severity::Info, vec![NodeKind::ROOT]),
        };
        let diags = Engine::lint(ast, &[&counter]);
        // check() is a no-op called once at the root; finish() walks the whole tree:
        // Root + Heading + Text + Paragraph + Text = 5.
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "5 nodes");
    }

    #[test]
    fn no_rules_yields_no_diagnostics() {
        let bytes = archive("text");
        let ast = tzlint_ast::access(&bytes).unwrap();
        assert!(Engine::lint(ast, &[]).is_empty());
    }

    #[test]
    fn lint_on_a_rootless_archive_is_safe() {
        // A degenerate archive whose root index has no node: the walk is skipped, no panic.
        let bytes = tzlint_ast::to_archive(&Ast {
            nodes: vec![],
            text: String::new(),
            root: NodeId(0),
        })
        .unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();
        let rule = FlagKind::new("flag-text", NodeKind::TEXT, "text");
        assert!(Engine::lint(ast, &[&rule]).is_empty());
    }
}
