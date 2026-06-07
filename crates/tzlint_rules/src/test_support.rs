//! Test-only helper: run one rule over a markdown source through the real engine.
//!
//! `tzlint_core` (the parser + engine) is a dev-dependency only — it is not part of the crate's
//! normal dependency graph, so the rules crate stays free of the engine (and of any future
//! `tzlint_core → tzlint_rules` cycle).

use tzlint_ast::morphology::{MorphologyBuilder, access_morphology, to_archive_morphology};
use tzlint_ast::{ArchivedAst, NodeId, NodeKind};
use tzlint_pdk::{Diagnostic, NodeRef, Rule};

/// Parse `source`, archive it, and run `rule` through `Engine::lint`, returning its diagnostics
/// (engine-sorted, kind-filtered exactly as in production).
pub(crate) fn diagnose(rule: &dyn Rule, source: &str) -> Vec<Diagnostic> {
    let ast = tzlint_core::parse(source).expect("test source parses");
    let bytes = tzlint_ast::to_archive(&ast).expect("archive");
    let archived = tzlint_ast::access(&bytes).expect("access");
    tzlint_core::Engine::lint(archived, None, &[rule])
}

/// The id of the first `PARAGRAPH` node in document order — discovered at runtime (never
/// hardcoded), so a parser renumber surfaces as a wrong/empty token set rather than silently.
///
/// "First" is decided by absolute byte offset ([`NodeRef::span`]`.start`), *not* by `NodeId`
/// value: the parser numbers nodes pre-order today, so the two coincide, but selecting on the
/// byte position keeps this helper honest to its "document order" contract even if a future
/// renumber breaks that coincidence.
pub(crate) fn first_paragraph_id(ast: &ArchivedAst) -> NodeId {
    let mut best: Option<(u32, NodeId)> = None;
    let mut stack: Vec<NodeRef> = NodeRef::root(ast).into_iter().collect();
    while let Some(node) = stack.pop() {
        if node.kind() == NodeKind::PARAGRAPH {
            let start = node.span().start;
            best = Some(match best {
                Some((best_start, best_id)) if best_start <= start => (best_start, best_id),
                _ => (start, node.id()),
            });
        }
        stack.extend(node.children());
    }
    best.map(|(_, id)| id).expect("test source has a paragraph")
}

/// Run a morphology-reading `rule` over `source` with a **synthetic** [`MorphologyV1`] table: the
/// `build` callback receives the first paragraph's [`NodeId`] and a builder, and pushes tokens keyed
/// to that node (surfaces are absolute byte offsets into the parsed paragraph text). The table is
/// archived and threaded through `Engine::lint` exactly as the real analysis pass does — so a
/// `with_morphology` rule actually fires without a real tokenizer backend.
///
/// **Fail-loud:** asserts the callback produced ≥1 token keyed to the paragraph, so a `NodeId`/offset
/// mismatch is a hard failure rather than a vacuously-green (zero-token) test.
pub(crate) fn diagnose_with_morphology(
    rule: &dyn Rule,
    source: &str,
    build: impl FnOnce(NodeId, &mut MorphologyBuilder),
) -> Vec<Diagnostic> {
    let ast = tzlint_core::parse(source).expect("test source parses");
    let bytes = tzlint_ast::to_archive(&ast).expect("archive");
    let archived = tzlint_ast::access(&bytes).expect("access");
    let pid = first_paragraph_id(archived);

    let mut builder = MorphologyBuilder::new();
    build(pid, &mut builder);
    let table = builder.finish();
    assert!(
        table.tokens.iter().any(|t| t.node == pid),
        "the build callback pushed no tokens keyed to the paragraph (NodeId/offset mismatch?)"
    );

    let mbytes = to_archive_morphology(&table).expect("archive morphology");
    let morph = access_morphology(&mbytes).expect("access morphology");
    tzlint_core::Engine::lint(archived, Some(morph), &[rule])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins [`first_paragraph_id`]'s "document order" contract: with two paragraphs it returns
    /// the byte-first one. Under today's pre-order `NodeId` numbering a min-`NodeId` selection
    /// would agree, so this can't *fail* against the current parser — it's a regression guard
    /// that keeps the byte-offset selection honest if a future renumber breaks that coincidence.
    #[test]
    fn first_paragraph_id_picks_the_byte_first_paragraph() {
        let ast = tzlint_core::parse("first para\n\nsecond para\n").expect("parses");
        let bytes = tzlint_ast::to_archive(&ast).expect("archive");
        let archived = tzlint_ast::access(&bytes).expect("access");

        let pid = first_paragraph_id(archived);
        let node = NodeRef::at(archived, pid).expect("the id resolves to a node");
        assert_eq!(node.kind(), NodeKind::PARAGRAPH);
        assert_eq!(node.span().start, 0, "must select the byte-first paragraph");
        assert_eq!(node.text(), "first para");
    }
}
