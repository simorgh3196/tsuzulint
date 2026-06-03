//! The single-traversal, multi-visitor lint engine.

use tzlint_ast::ArchivedAst;
use tzlint_ast::morphology::ArchivedMorphologyV1;
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
    ///
    /// `morphology` is the document's token table, when one is available. A rule that declares
    /// [`with_morphology`](tzlint_pdk::RuleMeta::with_morphology) reads it through its
    /// [`Context`]; the table is attached only to those rules' contexts (others never see it).
    /// When `morphology` is `None`, a rule that requires it is **skipped entirely** — neither its
    /// `check` nor its `finish` runs — rather than being fed an empty table and reporting nothing
    /// (or worse, spurious) findings. Rules that need no morphology are unaffected either way.
    #[must_use]
    pub fn lint(
        ast: &ArchivedAst,
        morphology: Option<&ArchivedMorphologyV1>,
        rules: &[&dyn Rule],
    ) -> Vec<Diagnostic> {
        // Hoist metadata once (not per node × rule); one context per rule.
        let metas: Vec<&tzlint_pdk::RuleMeta> = rules.iter().map(|rule| rule.meta()).collect();

        // A rule that needs morphology runs only when a table is available; otherwise it is
        // inactive and contributes nothing. `active[i]` gates both `check` and `finish` for
        // rule `i`, so a skipped rule never observes the document at all. This is a boolean
        // presence gate: matching a rule's `required_lang()` against the table's languages is
        // intentionally deferred to the M2h provider registry (which provisions only the
        // languages enabled rules need), so `required_lang()` is not consulted here yet.
        let active: Vec<bool> = metas
            .iter()
            .map(|meta| morphology.is_some() || !meta.needs_morphology())
            .collect();

        // One context per rule. Morphology is attached only to the rules that asked for it
        // (capability-scoped): a rule that did not declare it never sees the table.
        let mut contexts: Vec<Context> = metas
            .iter()
            .map(|meta| {
                let cx = Context::new(ast, meta.id.clone(), meta.default_severity);
                match morphology {
                    Some(table) if meta.needs_morphology() => cx.with_morphology(table),
                    _ => cx,
                }
            })
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
                    if active[index] && metas[index].visits(kind) {
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

        // Cross-node finalize, then aggregate and sort into the stable total order. A skipped
        // (inactive) rule's `finish` is skipped too, so it stays entirely dormant.
        for (index, cx) in contexts.iter_mut().enumerate() {
            if active[index] {
                rules[index].finish(cx);
            }
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
    use tzlint_ast::morphology::{Lang, access_morphology, to_archive_morphology};
    use tzlint_ast::{Ast, Node, NodeId, NodeKind, OptionNodeId, Span};
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

    /// A morphology-reading rule: declares [`with_morphology`](RuleMeta::with_morphology) and, at
    /// each visited node, reports how many tokens the table holds for it. The engine skips it
    /// entirely when no morphology table is available, rather than feeding it an empty one.
    struct MorphPeek {
        meta: RuleMeta,
    }
    impl MorphPeek {
        fn new(id: &str) -> Self {
            MorphPeek {
                meta: RuleMeta::new(id, Severity::Warning, vec![NodeKind::ROOT])
                    .with_morphology(Lang::JA),
            }
        }
    }
    impl Rule for MorphPeek {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            let n = cx.tokens_of(node.id()).count();
            cx.report(node.span(), format!("{n} tokens"));
        }
    }

    /// A morphology-requiring rule whose work is in `finish` (so it tests that a skipped rule's
    /// `finish` is skipped too, not just its `check`).
    struct MorphFinish {
        meta: RuleMeta,
    }
    impl MorphFinish {
        fn new(id: &str) -> Self {
            MorphFinish {
                meta: RuleMeta::new(id, Severity::Warning, vec![NodeKind::ROOT])
                    .with_morphology(Lang::JA),
            }
        }
    }
    impl Rule for MorphFinish {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, _node: NodeRef<'ast>, _cx: &mut Context<'ast>) {}
        fn finish<'ast>(&self, cx: &mut Context<'ast>) {
            cx.report(Span::new(0, 0), "finished");
        }
    }

    /// A rule that does NOT declare morphology but still *reads* its context, reporting what it
    /// observes. It pins the capability-scoping invariant: even when a table is passed to the
    /// engine, a non-declaring rule must see `morphology() == None` and no tokens.
    struct PlainObserver {
        meta: RuleMeta,
    }
    impl PlainObserver {
        fn new(id: &str) -> Self {
            PlainObserver {
                meta: RuleMeta::new(id, Severity::Warning, vec![NodeKind::ROOT]),
            }
        }
    }
    impl Rule for PlainObserver {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            cx.report(
                node.span(),
                format!(
                    "observed morph={} tokens={}",
                    cx.morphology().is_some(),
                    cx.tokens_of(node.id()).count()
                ),
            );
        }
    }

    fn archive(src: &str) -> tzlint_ast::AlignedVec {
        tzlint_ast::to_archive(&parse(src).unwrap()).unwrap()
    }

    /// An archived morphology table holding `count` tokens, all keyed to the root (`NodeId(0)`).
    fn morphology_on_root(count: u32) -> tzlint_ast::AlignedVec {
        use tzlint_ast::morphology::{MorphologyBuilder, Tagset, TokenAttrs};
        let mut builder = MorphologyBuilder::new();
        for i in 0..count {
            builder.push_token(
                TokenAttrs {
                    node: NodeId(0),
                    surface: Span::new(i, i + 1),
                    lang: Lang::JA,
                    tagset: Tagset::NONE,
                    flags: 0,
                },
                None,
                None,
                &[],
            );
        }
        to_archive_morphology(&builder.finish()).unwrap()
    }

    #[test]
    fn dispatches_only_to_registered_kinds() {
        // "# H\n\nbody" → Root, Heading, Text("H"), Paragraph, Text("body").
        let bytes = archive("# H\n\nbody");
        let ast = tzlint_ast::access(&bytes).unwrap();

        let text_rule = FlagKind::new("flag-text", NodeKind::TEXT, "text");
        let heading_rule = FlagKind::new("flag-heading", NodeKind::HEADING, "heading");
        let diags = Engine::lint(ast, None, &[&text_rule, &heading_rule]);

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
        let diags = Engine::lint(ast, None, &[&b, &a]); // pass b first on purpose
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
        let diags = Engine::lint(ast, None, &[&counter]);
        // check() is a no-op called once at the root; finish() walks the whole tree:
        // Root + Heading + Text + Paragraph + Text = 5.
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "5 nodes");
    }

    #[test]
    fn no_rules_yields_no_diagnostics() {
        let bytes = archive("text");
        let ast = tzlint_ast::access(&bytes).unwrap();
        assert!(Engine::lint(ast, None, &[]).is_empty());
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
        assert!(Engine::lint(ast, None, &[&rule]).is_empty());
    }

    #[test]
    fn threads_the_morphology_table_into_a_requesting_rule() {
        let bytes = archive("word");
        let ast = tzlint_ast::access(&bytes).unwrap();
        let mbytes = morphology_on_root(2); // 2 tokens keyed to the root
        let morph = access_morphology(&mbytes).unwrap();

        let rule = MorphPeek::new("peek");
        let diags = Engine::lint(ast, Some(morph), &[&rule]);
        // The rule visited the root, read the table through its context, and saw both tokens.
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "2 tokens");
    }

    #[test]
    fn skips_a_morphology_rule_when_no_table_is_available() {
        let bytes = archive("word");
        let ast = tzlint_ast::access(&bytes).unwrap();

        let morph_rule = MorphPeek::new("peek"); // needs morphology
        let plain = FlagKind::new("flag-text", NodeKind::TEXT, "text"); // does not
        let diags = Engine::lint(ast, None, &[&morph_rule, &plain]);

        // The morphology rule is skipped entirely (no "N tokens" diagnostic, nothing under its id),
        // while the plain rule runs exactly as before.
        assert!(diags.iter().all(|d| d.rule_id.as_str() != "peek"));
        assert!(diags.iter().any(|d| d.rule_id.as_str() == "flag-text"));
    }

    #[test]
    fn a_skipped_morphology_rule_does_not_run_its_finish() {
        let bytes = archive("word");
        let ast = tzlint_ast::access(&bytes).unwrap();
        let rule = MorphFinish::new("mf");

        // Absent table → the rule is inactive, so even its `finish` must not emit.
        assert!(Engine::lint(ast, None, &[&rule]).is_empty());

        // Present table (even an empty one) → the rule is active and `finish` runs.
        let mbytes = morphology_on_root(0);
        let morph = access_morphology(&mbytes).unwrap();
        let diags = Engine::lint(ast, Some(morph), &[&rule]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "finished");
    }

    #[test]
    fn a_non_morphology_rule_runs_whether_or_not_a_table_is_present() {
        let bytes = archive("# H\n\nbody");
        let ast = tzlint_ast::access(&bytes).unwrap();
        let rule = FlagKind::new("flag-text", NodeKind::TEXT, "text");

        let without = Engine::lint(ast, None, &[&rule]);
        let mbytes = morphology_on_root(3);
        let morph = access_morphology(&mbytes).unwrap();
        let with = Engine::lint(ast, Some(morph), &[&rule]);

        // A rule that never declared morphology is unaffected by the table's presence.
        assert_eq!(without.len(), 2);
        assert_eq!(with.len(), without.len());
        assert!(with.iter().all(|d| d.message == "text"));
    }

    #[test]
    fn morphology_is_routed_per_rule_across_a_mixed_rule_set_with_a_present_table() {
        // The realistic shape: several rules in one walk with the table available — two distinct
        // morphology rules (check-based and finish-based) interleaved with non-declaring rules.
        // This pins, in a single call: (a) each morphology rule gets its own table-attached
        // context (index alignment across multiple active rules), and (b) capability-scoping —
        // a non-declaring rule must NOT see the table even though it was passed to the engine.
        let bytes = archive("word");
        let ast = tzlint_ast::access(&bytes).unwrap();
        let mbytes = morphology_on_root(2);
        let morph = access_morphology(&mbytes).unwrap();

        let peek = MorphPeek::new("peek"); // declares morphology, check-based
        let plain = PlainObserver::new("plain"); // declares nothing, but reads its context
        let finish = MorphFinish::new("mf"); // declares morphology, finish-based
        let diags = Engine::lint(ast, Some(morph), &[&peek, &plain, &finish]);

        let msg = |id: &str| {
            diags
                .iter()
                .find(|d| d.rule_id.as_str() == id)
                .map(|d| d.message.as_str())
        };
        // Both morphology rules ran and saw the attached table; the finish-based one fired too.
        assert_eq!(msg("peek"), Some("2 tokens"));
        assert_eq!(msg("mf"), Some("finished"));
        // The non-declaring rule ran but observed an empty (None) context — the table was NOT
        // attached to it. (If the engine's `if meta.needs_morphology()` scope guard were dropped,
        // this would read `morph=true tokens=2` and the assertion would fail.)
        assert_eq!(msg("plain"), Some("observed morph=false tokens=0"));
    }

    #[test]
    fn the_walk_is_cycle_safe_on_a_malformed_archive() {
        // A byte-valid but malformed archive whose child links form a cycle: node 2's first_child
        // points back to node 1, which the walk has already visited. `access` validates byte types
        // but not the link graph, so this is constructible. The engine's `visited` bitmap must
        // skip the revisit (engine.rs's already-visited guard) — terminating with no hang and
        // visiting each node exactly once (so the PARAGRAPH rule fires once, not twice).
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
                    first_child: OptionNodeId::some(NodeId(2)),
                    next_sibling: OptionNodeId::NONE,
                },
                Node {
                    kind: NodeKind::TEXT,
                    span: Span::new(0, 1),
                    parent: NodeId(1),
                    first_child: OptionNodeId::some(NodeId(1)), // cycle back to node 1
                    next_sibling: OptionNodeId::NONE,
                },
            ],
            text: String::from("x"),
            root: NodeId(0),
        };
        let bytes = tzlint_ast::to_archive(&ast).unwrap();
        let archived = tzlint_ast::access(&bytes).unwrap();

        let rule = FlagKind::new("flag-para", NodeKind::PARAGRAPH, "para");
        let diags = Engine::lint(archived, None, &[&rule]);
        // Node 1 is reachable from both the root and (cyclically) from node 2, but the bitmap
        // visits it once → exactly one diagnostic, and the call returns (no infinite loop).
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "para");
    }
}
