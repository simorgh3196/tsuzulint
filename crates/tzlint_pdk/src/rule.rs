//! The [`Rule`] trait and the per-rule [`Context`] the engine drives.

use alloc::string::String;
use alloc::vec::Vec;

use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken};
use tzlint_ast::{ArchivedAst, NodeId, Span};

use crate::{Diagnostic, Fix, NodeRef, RuleId, RuleMeta, Severity};

/// Per-rule, per-file scratch the engine hands to a rule during the single walk.
///
/// It is **rule-scoped** and persists across that rule's [`Rule::check`] calls, so a
/// cross-node rule can accumulate state and emit from [`Rule::finish`]. Reported
/// diagnostics are tagged with the rule's id and effective severity automatically.
/// Accumulation lives here (not in `&self`) because rule instances are shared across
/// worker threads.
pub struct Context<'ast> {
    ast: &'ast ArchivedAst,
    morphology: Option<&'ast ArchivedMorphologyV1>,
    rule_id: RuleId,
    severity: Severity,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Context<'ast> {
    /// Create a context for `rule_id` reporting at `severity` over `ast`. The engine builds
    /// one of these per rule. Morphology is absent by default; the engine attaches it with
    /// [`with_morphology`](Self::with_morphology) for the rules that ask for it.
    pub fn new(ast: &'ast ArchivedAst, rule_id: RuleId, severity: Severity) -> Self {
        Context {
            ast,
            morphology: None,
            rule_id,
            severity,
            diagnostics: Vec::new(),
        }
    }

    /// Attach the morphology table for this document (consumed builder-style by the engine).
    ///
    /// Only rules that declare [`RuleMeta::with_morphology`](crate::RuleMeta::with_morphology)
    /// get a context with morphology attached; everyone else sees [`morphology`](Self::morphology)
    /// as `None` and [`tokens_of`](Self::tokens_of) as empty.
    #[must_use]
    pub fn with_morphology(mut self, morphology: &'ast ArchivedMorphologyV1) -> Self {
        self.morphology = Some(morphology);
        self
    }

    /// The archive, for a cross-node rule that self-traverses via [`NodeRef`].
    pub fn ast(&self) -> &'ast ArchivedAst {
        self.ast
    }

    /// The morphology table for this document, if one was attached (see
    /// [`with_morphology`](Self::with_morphology)). `None` means morphology is unavailable —
    /// a rule that needs it should declare so in its [`RuleMeta`](crate::RuleMeta) and the
    /// engine will not run it against an absent table.
    pub fn morphology(&self) -> Option<&'ast ArchivedMorphologyV1> {
        self.morphology
    }

    /// The morphological tokens for `node`, in order. Empty when no morphology table is
    /// attached or when the node has no tokens — never panics, so a rule can call it
    /// unconditionally.
    pub fn tokens_of(&self, node: NodeId) -> impl Iterator<Item = &'ast ArchivedToken> {
        self.morphology
            .into_iter()
            .flat_map(move |m| m.tokens_of(node))
    }

    /// Report a problem at `span`. The rule id and severity are filled in automatically.
    pub fn report(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(
            self.rule_id.clone(),
            self.severity,
            span,
            message,
        ));
    }

    /// Report a problem at `span` carrying suggested [`Fix`]es.
    pub fn report_with_fixes(
        &mut self,
        span: Span,
        message: impl Into<String>,
        fixes: impl IntoIterator<Item = Fix>,
    ) {
        let mut diagnostic = Diagnostic::new(self.rule_id.clone(), self.severity, span, message);
        diagnostic.fixes.extend(fixes);
        self.diagnostics.push(diagnostic);
    }

    /// Consume the accumulated diagnostics (the engine calls this after the walk).
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

/// A lint rule. Native rules — and, later, plugin shims — implement this.
///
/// During the engine's **single traversal**, [`check`](Rule::check) is invoked once per
/// node whose kind is in [`RuleMeta::node_kinds`]. [`finish`](Rule::finish) runs once after
/// the walk, for cross-node rules that accumulate in the [`Context`]. `&self` is immutable
/// (rule instances are shared across workers); all per-file state lives in the `Context`.
///
/// The `Send + Sync` bound expresses that sharing: rule instances are `Arc`-shared across
/// the (future) rayon workers that lint files in parallel.
pub trait Rule: Send + Sync {
    /// Static metadata: id, the node kinds to visit, and default severity.
    fn meta(&self) -> &RuleMeta;

    /// Inspect one matching node, reporting problems via `cx`.
    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>);

    /// Optional finalize after the walk (for cross-node / document-level rules). Default:
    /// no-op.
    fn finish<'ast>(&self, _cx: &mut Context<'ast>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morphology::{MorphologyProvider, WhitespaceProvider};
    use alloc::vec;
    use tzlint_ast::morphology::{Lang, access_morphology, to_archive_morphology};
    use tzlint_ast::{Ast, Node, NodeId, NodeKind, OptionNodeId};

    /// A one-node archive ("hello world" under a single ROOT) to hang a context on.
    fn sample_ast() -> Ast {
        Ast {
            nodes: vec![Node {
                kind: NodeKind::ROOT,
                span: Span::new(0, 11),
                parent: NodeId(0),
                first_child: OptionNodeId::NONE,
                next_sibling: OptionNodeId::NONE,
            }],
            text: alloc::string::String::from("hello world"),
            root: NodeId(0),
        }
    }

    #[test]
    fn context_without_morphology_has_none_and_no_tokens() {
        let bytes = tzlint_ast::to_archive(&sample_ast()).unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();

        let cx = Context::new(ast, "r".into(), Severity::Warning);
        assert!(cx.morphology().is_none());
        assert_eq!(cx.tokens_of(NodeId(0)).count(), 0);
    }

    #[test]
    fn context_with_morphology_exposes_table_and_per_node_tokens() {
        let bytes = tzlint_ast::to_archive(&sample_ast()).unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();

        // Two whitespace-separated tokens ("hello", "world"), both keyed to NodeId(0).
        let morph = WhitespaceProvider::new(Lang::JA)
            .analyze("hello world", 0, NodeId(0))
            .unwrap();
        let mbytes = to_archive_morphology(&morph).unwrap();
        let archived_morph = access_morphology(&mbytes).unwrap();

        let cx = Context::new(ast, "r".into(), Severity::Warning).with_morphology(archived_morph);
        assert!(cx.morphology().is_some());
        assert_eq!(cx.tokens_of(NodeId(0)).count(), 2);
        // A node with no tokens in the table reads back as empty, not a panic.
        assert_eq!(cx.tokens_of(NodeId(1)).count(), 0);
    }

    #[test]
    fn tokens_of_filters_to_the_requested_node_in_a_multi_node_table() {
        use tzlint_ast::morphology::{MorphologyBuilder, Tagset, TokenAttrs};

        let bytes = tzlint_ast::to_archive(&sample_ast()).unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();

        // Hand-build a table holding tokens for two distinct nodes: 2 for NodeId(0), 3 for
        // NodeId(2). `tokens_of` must return only the requested node's tokens, in order.
        let mut builder = MorphologyBuilder::new();
        for (start, end) in [(0u32, 2u32), (2, 4)] {
            builder.push_token(
                TokenAttrs {
                    node: NodeId(0),
                    surface: Span::new(start, end),
                    lang: Lang::JA,
                    tagset: Tagset::NONE,
                    flags: 0,
                },
                None,
                None,
                &[],
            );
        }
        for (start, end) in [(4u32, 5u32), (5, 6), (6, 7)] {
            builder.push_token(
                TokenAttrs {
                    node: NodeId(2),
                    surface: Span::new(start, end),
                    lang: Lang::JA,
                    tagset: Tagset::NONE,
                    flags: 0,
                },
                None,
                None,
                &[],
            );
        }
        let morph = builder.finish();
        let mbytes = to_archive_morphology(&morph).unwrap();
        let archived_morph = access_morphology(&mbytes).unwrap();

        let cx = Context::new(ast, "r".into(), Severity::Warning).with_morphology(archived_morph);
        assert_eq!(cx.tokens_of(NodeId(0)).count(), 2);
        assert_eq!(cx.tokens_of(NodeId(2)).count(), 3);
        assert_eq!(cx.tokens_of(NodeId(1)).count(), 0); // a node with no tokens

        // The filter returns exactly NodeId(0)'s surfaces, in push order (content check).
        let spans: Vec<_> = cx.tokens_of(NodeId(0)).map(|t| t.surface()).collect();
        assert_eq!(spans, vec![Span::new(0, 2), Span::new(2, 4)]);
    }
}
