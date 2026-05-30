//! The [`Rule`] trait and the per-rule [`Context`] the engine drives.

use alloc::string::String;
use alloc::vec::Vec;

use tzlint_ast::{ArchivedAst, Span};

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
    rule_id: RuleId,
    severity: Severity,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Context<'ast> {
    /// Create a context for `rule_id` reporting at `severity` over `ast`. The engine builds
    /// one of these per rule.
    pub fn new(ast: &'ast ArchivedAst, rule_id: RuleId, severity: Severity) -> Self {
        Context {
            ast,
            rule_id,
            severity,
            diagnostics: Vec::new(),
        }
    }

    /// The archive, for a cross-node rule that self-traverses via [`NodeRef`].
    pub fn ast(&self) -> &'ast ArchivedAst {
        self.ast
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
pub trait Rule {
    /// Static metadata: id, the node kinds to visit, and default severity.
    fn meta(&self) -> &RuleMeta;

    /// Inspect one matching node, reporting problems via `cx`.
    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>);

    /// Optional finalize after the walk (for cross-node / document-level rules). Default:
    /// no-op.
    fn finish<'ast>(&self, _cx: &mut Context<'ast>) {}
}
