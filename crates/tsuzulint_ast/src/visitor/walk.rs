//! Walk functions for AST traversal.
//!
//! These functions provide the traversal logic for the Visitor pattern.
//! They are used by the default implementations in `Visitor` trait.

use std::ops::ControlFlow;

use crate::{NodeType, TxtNode};

use super::visit::{VisitResult, Visitor};

/// Walks a node by dispatching to the appropriate type-specific visitor method.
///
/// This function:
/// 1. Calls `enter_node` on the visitor
/// 2. Dispatches to the appropriate `visit_*` method based on node type
/// 3. Calls `exit_node` on the visitor
///
/// # Arguments
///
/// * `visitor` - The visitor to use
/// * `node` - The node to visit
///
/// # Returns
///
/// `ControlFlow::Continue(())` to continue traversal, or `ControlFlow::Break(())` to stop.
pub fn walk_node<'a, V>(visitor: &mut V, node: &TxtNode<'a>) -> VisitResult
where
    V: Visitor<'a>,
{
    // Enter hook
    visitor.enter_node(node)?;

    // Dispatch to type-specific method
    let result = match node.node_type {
        // Block-level nodes
        NodeType::Document => visitor.visit_document(node),
        NodeType::Paragraph => visitor.visit_paragraph(node),
        NodeType::Header => visitor.visit_header(node),
        NodeType::BlockQuote => visitor.visit_block_quote(node),
        NodeType::List => visitor.visit_list(node),
        NodeType::ListItem => visitor.visit_list_item(node),
        NodeType::CodeBlock => visitor.visit_code_block(node),
        NodeType::HorizontalRule => visitor.visit_horizontal_rule(node),
        NodeType::Html => visitor.visit_html(node),

        // Inline-level nodes
        NodeType::Str => visitor.visit_str(node),
        NodeType::Break => visitor.visit_break(node),
        NodeType::Emphasis => visitor.visit_emphasis(node),
        NodeType::Strong => visitor.visit_strong(node),
        NodeType::Delete => visitor.visit_delete(node),
        NodeType::Code => visitor.visit_code(node),
        NodeType::Link => visitor.visit_link(node),
        NodeType::Image => visitor.visit_image(node),

        // Reference nodes
        NodeType::LinkReference => visitor.visit_link_reference(node),
        NodeType::ImageReference => visitor.visit_image_reference(node),
        NodeType::Definition => visitor.visit_definition(node),

        // Table nodes (GFM)
        NodeType::Table => visitor.visit_table(node),
        NodeType::TableRow => visitor.visit_table_row(node),
        NodeType::TableCell => visitor.visit_table_cell(node),

        // Footnote nodes
        NodeType::FootnoteDefinition => visitor.visit_footnote_definition(node),
        NodeType::FootnoteReference => visitor.visit_footnote_reference(node),
    };

    result?;

    // Exit hook
    visitor.exit_node(node)
}

/// Walks all children of a node.
///
/// This function iterates over `node.children` and calls `walk_node` for each child.
/// It supports early termination via `ControlFlow::Break`.
///
/// # Arguments
///
/// * `visitor` - The visitor to use
/// * `node` - The parent node whose children to visit
///
/// # Returns
///
/// `ControlFlow::Continue(())` if all children were visited,
/// or `ControlFlow::Break(())` if traversal was stopped early.
#[inline]
pub fn walk_children<'a, V>(visitor: &mut V, node: &TxtNode<'a>) -> VisitResult
where
    V: Visitor<'a>,
{
    for child in node.children {
        walk_node(visitor, child)?;
    }
    ControlFlow::Continue(())
}

/// Recursively collects every descendant node (including `root` itself) whose
/// `node_type` matches one of `types`.
///
/// Used by the plugin dispatch layer to honour a WASM rule manifest's
/// `node_types` filter: when the parser hands the dispatcher a block-level
/// node, the dispatcher needs the descendants of that block whose type the
/// rule actually wants to see (e.g. all `Str` descendants of a `Paragraph`).
/// Without this, `node.type == "Str"`-style rules receive the block itself,
/// take the early-return path, and silently produce zero diagnostics.
///
/// `types` is matched against `NodeType`'s `Display` form so the comparison
/// agrees with the textlint-compatible names used in rule manifests
/// (`"Str"`, `"Paragraph"`, `"Header"`, ...).
pub fn collect_nodes_by_type<'a, 'tree>(
    root: &'tree TxtNode<'a>,
    types: &[String],
    out: &mut Vec<&'tree TxtNode<'a>>,
) {
    if types.iter().any(|t| t == &root.node_type.to_string()) {
        out.push(root);
    }
    for child in root.children {
        collect_nodes_by_type(child, types, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AstArena, Span};

    /// A simple visitor that counts nodes of each type.
    struct NodeCounter {
        document_count: usize,
        paragraph_count: usize,
        str_count: usize,
        total_count: usize,
    }

    impl NodeCounter {
        fn new() -> Self {
            Self {
                document_count: 0,
                paragraph_count: 0,
                str_count: 0,
                total_count: 0,
            }
        }
    }

    impl<'a> Visitor<'a> for NodeCounter {
        fn enter_node(&mut self, _node: &TxtNode<'a>) -> VisitResult {
            self.total_count += 1;
            ControlFlow::Continue(())
        }

        fn visit_document(&mut self, node: &TxtNode<'a>) -> VisitResult {
            self.document_count += 1;
            walk_children(self, node)
        }

        fn visit_paragraph(&mut self, node: &TxtNode<'a>) -> VisitResult {
            self.paragraph_count += 1;
            walk_children(self, node)
        }

        fn visit_str(&mut self, _node: &TxtNode<'a>) -> VisitResult {
            self.str_count += 1;
            ControlFlow::Continue(())
        }
    }

    #[test]
    fn walk_node_visits_single_text_node() {
        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));

        let mut counter = NodeCounter::new();
        let result = walk_node(&mut counter, text);

        assert!(result.is_continue());
        assert_eq!(counter.str_count, 1);
        assert_eq!(counter.total_count, 1);
    }

    #[test]
    fn walk_node_visits_parent_and_children() {
        let arena = AstArena::new();
        let text1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let text2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 11), "world"));
        let children = arena.alloc_slice_copy(&[*text1, *text2]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 11),
            children,
        ));

        let mut counter = NodeCounter::new();
        let result = walk_node(&mut counter, para);

        assert!(result.is_continue());
        assert_eq!(counter.paragraph_count, 1);
        assert_eq!(counter.str_count, 2);
        assert_eq!(counter.total_count, 3);
    }

    #[test]
    fn walk_node_visits_nested_structure() {
        let arena = AstArena::new();

        // Create: Document -> Paragraph -> [Str, Str]
        let text1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let text2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 11), "world"));
        let para_children = arena.alloc_slice_copy(&[*text1, *text2]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 11),
            para_children,
        ));
        let doc_children = arena.alloc_slice_copy(&[*para]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 11),
            doc_children,
        ));

        let mut counter = NodeCounter::new();
        let result = walk_node(&mut counter, doc);

        assert!(result.is_continue());
        assert_eq!(counter.document_count, 1);
        assert_eq!(counter.paragraph_count, 1);
        assert_eq!(counter.str_count, 2);
        assert_eq!(counter.total_count, 4);
    }

    /// A visitor that stops after finding the first Str node.
    struct FirstStrFinder<'a> {
        found: Option<&'a str>,
    }

    impl<'a> Visitor<'a> for FirstStrFinder<'a> {
        fn visit_str(&mut self, node: &TxtNode<'a>) -> VisitResult {
            if let Some(text) = node.value {
                self.found = Some(text);
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        }
    }

    #[test]
    fn walk_node_supports_early_termination() {
        let arena = AstArena::new();
        let text1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "first"));
        let text2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 12), "second"));
        let children = arena.alloc_slice_copy(&[*text1, *text2]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 12),
            children,
        ));

        let mut finder = FirstStrFinder { found: None };
        let result = walk_node(&mut finder, para);

        assert!(result.is_break());
        assert_eq!(finder.found, Some("first"));
    }

    #[test]
    fn walk_children_empty_children() {
        let arena = AstArena::new();
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 0),
            &[],
        ));

        let mut counter = NodeCounter::new();
        let result = walk_children(&mut counter, para);

        assert!(result.is_continue());
        assert_eq!(counter.total_count, 0);
    }

    /// A visitor that collects text content.
    struct TextCollector<'a> {
        texts: Vec<&'a str>,
    }

    impl<'a> Visitor<'a> for TextCollector<'a> {
        fn visit_str(&mut self, node: &TxtNode<'a>) -> VisitResult {
            if let Some(text) = node.value {
                self.texts.push(text);
            }
            ControlFlow::Continue(())
        }
    }

    #[test]
    fn visitor_collects_text_content() {
        let arena = AstArena::new();
        let text1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let text2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 7), " "));
        let text3 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(7, 12), "world"));
        let children = arena.alloc_slice_copy(&[*text1, *text2, *text3]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 12),
            children,
        ));

        let mut collector = TextCollector { texts: Vec::new() };
        let _ = walk_node(&mut collector, para);

        assert_eq!(collector.texts, vec!["hello", " ", "world"]);
    }

    #[test]
    fn walk_node_calls_enter_and_exit_hooks() {
        struct HookTracker {
            events: Vec<String>,
        }

        impl<'a> Visitor<'a> for HookTracker {
            fn enter_node(&mut self, node: &TxtNode<'a>) -> VisitResult {
                self.events.push(format!("enter:{}", node.node_type));
                ControlFlow::Continue(())
            }

            fn exit_node(&mut self, node: &TxtNode<'a>) -> VisitResult {
                self.events.push(format!("exit:{}", node.node_type));
                ControlFlow::Continue(())
            }
        }

        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let children = arena.alloc_slice_copy(&[*text]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 5),
            children,
        ));

        let mut tracker = HookTracker { events: Vec::new() };
        let _ = walk_node(&mut tracker, para);

        assert_eq!(
            tracker.events,
            vec!["enter:Paragraph", "enter:Str", "exit:Str", "exit:Paragraph"]
        );
    }
}

#[cfg(test)]
mod collect_tests {
    use super::*;
    use crate::{AstArena, Span};

    fn doc<'a>(arena: &'a AstArena) -> TxtNode<'a> {
        // Document
        //   Header [Str("title")]
        //   Paragraph [Str("hello"), Emphasis [Str("world")]]
        let title_str = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(2, 7), "title"));
        let hdr = arena.alloc(TxtNode::new_parent(
            NodeType::Header,
            Span::new(0, 7),
            arena.alloc_slice_copy(&[*title_str]),
        ));
        let hello = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(9, 14), "hello"));
        let world = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(16, 21), "world"));
        let em = arena.alloc(TxtNode::new_parent(
            NodeType::Emphasis,
            Span::new(15, 22),
            arena.alloc_slice_copy(&[*world]),
        ));
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(8, 22),
            arena.alloc_slice_copy(&[*hello, *em]),
        ));
        TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 22),
            arena.alloc_slice_copy(&[*hdr, *para]),
        )
    }

    #[test]
    fn collect_str_descendants_of_paragraph() {
        let arena = AstArena::new();
        let document = doc(&arena);
        let paragraph = &document.children[1];

        let mut out = Vec::new();
        collect_nodes_by_type(paragraph, &["Str".to_string()], &mut out);

        assert_eq!(out.len(), 2, "expected both Str leaves under Paragraph");
        for node in out {
            assert_eq!(node.node_type, NodeType::Str);
        }
    }

    #[test]
    fn collect_includes_root_when_matching() {
        let arena = AstArena::new();
        let document = doc(&arena);
        let paragraph = &document.children[1];

        let mut out = Vec::new();
        collect_nodes_by_type(paragraph, &["Paragraph".to_string()], &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].node_type, NodeType::Paragraph);
    }

    #[test]
    fn collect_multiple_types() {
        let arena = AstArena::new();
        let document = doc(&arena);

        let mut out = Vec::new();
        collect_nodes_by_type(
            &document,
            &["Header".to_string(), "Emphasis".to_string()],
            &mut out,
        );

        assert_eq!(out.len(), 2);
    }

    #[test]
    fn collect_empty_filter_yields_nothing() {
        let arena = AstArena::new();
        let document = doc(&arena);

        let mut out = Vec::new();
        collect_nodes_by_type(&document, &[], &mut out);

        assert!(out.is_empty());
    }
}
