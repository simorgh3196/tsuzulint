//! MutVisitor trait for transforming TxtAST nodes.
//!
//! This module provides the `MutVisitor` trait for AST transformation.
//! Due to arena allocation constraints, transformations produce new nodes
//! rather than modifying in place.
//!
//! # Example
//!
//! ```rust
//! use texide_ast::{AstArena, TxtNode, NodeType, Span};
//! use texide_ast::visitor::{MutVisitor, walk_node_mut};
//!
//! /// Transforms all text to uppercase.
//! struct UppercaseText<'a> {
//!     arena: &'a AstArena,
//! }
//!
//! impl<'a> MutVisitor<'a> for UppercaseText<'a> {
//!     fn arena(&self) -> &'a AstArena {
//!         self.arena
//!     }
//!
//!     fn visit_str_mut(&mut self, node: &TxtNode<'a>) -> Option<TxtNode<'a>> {
//!         node.value.map(|text| {
//!             let upper = self.arena.alloc_str(&text.to_uppercase());
//!             TxtNode::new_text(node.node_type, node.span, upper)
//!         })
//!     }
//! }
//! ```

use crate::{AstArena, NodeType, TxtNode};

/// Result type for mutable visitor methods.
///
/// - `Some(node)` - The node was transformed, use the new node
/// - `None` - The node was not changed
pub type VisitMutResult<'a> = Option<TxtNode<'a>>;

/// Visitor trait for transforming TxtAST nodes.
///
/// Unlike `Visitor`, `MutVisitor` allows modification of visited nodes.
/// Due to arena allocation constraints, transformations produce new nodes
/// rather than modifying in place.
///
/// # Limitations
///
/// Because TxtNode uses arena allocation with `Copy` semantics,
/// true in-place mutation is not possible. Instead, MutVisitor
/// methods return `Option<TxtNode<'a>>` to indicate whether a
/// node should be replaced.
///
/// # Lifetime
///
/// The `'a` lifetime ties the visitor to its arena allocator.
pub trait MutVisitor<'a>: Sized {
    /// Returns a reference to the arena for allocating new nodes.
    fn arena(&self) -> &'a AstArena;

    /// Transforms a node, returning `Some(new_node)` if replaced, `None` if unchanged.
    fn visit_node_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_node_mut(self, node)
    }

    // === Block-level node visitors ===

    /// Transform a Document node.
    fn visit_document_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a Paragraph node.
    fn visit_paragraph_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a Header node.
    fn visit_header_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a BlockQuote node.
    fn visit_block_quote_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a List node.
    fn visit_list_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a ListItem node.
    fn visit_list_item_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a CodeBlock node.
    fn visit_code_block_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    /// Transform a HorizontalRule node.
    fn visit_horizontal_rule_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    /// Transform an Html node.
    fn visit_html_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    // === Inline-level node visitors ===

    /// Transform a Str (text) node.
    fn visit_str_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    /// Transform a Break node.
    fn visit_break_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    /// Transform an Emphasis node.
    fn visit_emphasis_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a Strong node.
    fn visit_strong_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a Delete node.
    fn visit_delete_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a Code (inline) node.
    fn visit_code_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    /// Transform a Link node.
    fn visit_link_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform an Image node.
    fn visit_image_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    // === Reference node visitors ===

    /// Transform a LinkReference node.
    fn visit_link_reference_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform an ImageReference node.
    fn visit_image_reference_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    /// Transform a Definition node.
    fn visit_definition_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }

    // === Table node visitors (GFM) ===

    /// Transform a Table node.
    fn visit_table_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a TableRow node.
    fn visit_table_row_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a TableCell node.
    fn visit_table_cell_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    // === Footnote node visitors ===

    /// Transform a FootnoteDefinition node.
    fn visit_footnote_definition_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
        walk_children_mut(self, node)
    }

    /// Transform a FootnoteReference node.
    fn visit_footnote_reference_mut(&mut self, _node: &TxtNode<'a>) -> VisitMutResult<'a> {
        None // No change by default
    }
}

/// Walks a node for mutation, returning a new node if any changes were made.
///
/// This function dispatches to the appropriate `visit_*_mut` method based on node type.
pub fn walk_node_mut<'a, V>(visitor: &mut V, node: &TxtNode<'a>) -> VisitMutResult<'a>
where
    V: MutVisitor<'a>,
{
    match node.node_type {
        // Block-level nodes
        NodeType::Document => visitor.visit_document_mut(node),
        NodeType::Paragraph => visitor.visit_paragraph_mut(node),
        NodeType::Header => visitor.visit_header_mut(node),
        NodeType::BlockQuote => visitor.visit_block_quote_mut(node),
        NodeType::List => visitor.visit_list_mut(node),
        NodeType::ListItem => visitor.visit_list_item_mut(node),
        NodeType::CodeBlock => visitor.visit_code_block_mut(node),
        NodeType::HorizontalRule => visitor.visit_horizontal_rule_mut(node),
        NodeType::Html => visitor.visit_html_mut(node),

        // Inline-level nodes
        NodeType::Str => visitor.visit_str_mut(node),
        NodeType::Break => visitor.visit_break_mut(node),
        NodeType::Emphasis => visitor.visit_emphasis_mut(node),
        NodeType::Strong => visitor.visit_strong_mut(node),
        NodeType::Delete => visitor.visit_delete_mut(node),
        NodeType::Code => visitor.visit_code_mut(node),
        NodeType::Link => visitor.visit_link_mut(node),
        NodeType::Image => visitor.visit_image_mut(node),

        // Reference nodes
        NodeType::LinkReference => visitor.visit_link_reference_mut(node),
        NodeType::ImageReference => visitor.visit_image_reference_mut(node),
        NodeType::Definition => visitor.visit_definition_mut(node),

        // Table nodes (GFM)
        NodeType::Table => visitor.visit_table_mut(node),
        NodeType::TableRow => visitor.visit_table_row_mut(node),
        NodeType::TableCell => visitor.visit_table_cell_mut(node),

        // Footnote nodes
        NodeType::FootnoteDefinition => visitor.visit_footnote_definition_mut(node),
        NodeType::FootnoteReference => visitor.visit_footnote_reference_mut(node),
    }
}

/// Walks children for mutation, returning a new node if any children changed.
///
/// This function iterates over `node.children`, applies `visit_node_mut` to each,
/// and returns a new node with updated children if any were transformed.
pub fn walk_children_mut<'a, V>(visitor: &mut V, node: &TxtNode<'a>) -> VisitMutResult<'a>
where
    V: MutVisitor<'a>,
{
    if node.children.is_empty() {
        return None;
    }

    let mut changed = false;
    let mut new_children: Vec<TxtNode<'a>> = Vec::with_capacity(node.children.len());

    for child in node.children {
        if let Some(new_child) = visitor.visit_node_mut(child) {
            new_children.push(new_child);
            changed = true;
        } else {
            new_children.push(*child);
        }
    }

    if changed {
        let children = visitor.arena().alloc_slice_clone(&new_children);
        Some(TxtNode {
            node_type: node.node_type,
            span: node.span,
            children,
            value: node.value,
            data: node.data,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;

    /// A visitor that transforms text to uppercase.
    struct UppercaseTransformer<'a> {
        arena: &'a AstArena,
    }

    impl<'a> MutVisitor<'a> for UppercaseTransformer<'a> {
        fn arena(&self) -> &'a AstArena {
            self.arena
        }

        fn visit_str_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
            node.value.map(|text| {
                let upper = self.arena.alloc_str(&text.to_uppercase());
                TxtNode::new_text(node.node_type, node.span, upper)
            })
        }
    }

    #[test]
    fn walk_node_mut_transforms_text_node() {
        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));

        let mut transformer = UppercaseTransformer { arena: &arena };
        let result = walk_node_mut(&mut transformer, text);

        assert!(result.is_some());
        let new_node = result.unwrap();
        assert_eq!(new_node.value, Some("HELLO"));
    }

    #[test]
    fn walk_children_mut_transforms_nested_text() {
        let arena = AstArena::new();
        let text1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let text2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 11), "world"));
        let children = arena.alloc_slice_copy(&[*text1, *text2]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 11),
            children,
        ));

        let mut transformer = UppercaseTransformer { arena: &arena };
        let result = walk_node_mut(&mut transformer, para);

        assert!(result.is_some());
        let new_para = result.unwrap();
        assert_eq!(new_para.children.len(), 2);
        assert_eq!(new_para.children[0].value, Some("HELLO"));
        assert_eq!(new_para.children[1].value, Some("WORLD"));
    }

    #[test]
    fn walk_children_mut_returns_none_when_no_changes() {
        let arena = AstArena::new();
        let hr = arena.alloc(TxtNode::new_leaf(NodeType::HorizontalRule, Span::new(0, 3)));
        let children = arena.alloc_slice_copy(&[*hr]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 3),
            children,
        ));

        let mut transformer = UppercaseTransformer { arena: &arena };
        let result = walk_node_mut(&mut transformer, doc);

        // No text nodes to transform, so should return None
        assert!(result.is_none());
    }

    #[test]
    fn walk_children_mut_empty_children() {
        let arena = AstArena::new();
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 0),
            &[],
        ));

        let mut transformer = UppercaseTransformer { arena: &arena };
        let result = walk_children_mut(&mut transformer, para);

        assert!(result.is_none());
    }

    /// A visitor that adjusts header depth.
    struct HeaderDepthAdjuster<'a> {
        arena: &'a AstArena,
        offset: i8,
    }

    impl<'a> MutVisitor<'a> for HeaderDepthAdjuster<'a> {
        fn arena(&self) -> &'a AstArena {
            self.arena
        }

        fn visit_header_mut(&mut self, node: &TxtNode<'a>) -> VisitMutResult<'a> {
            // First, transform children
            let base = walk_children_mut(self, node);
            let mut new_node = base.unwrap_or(*node);

            // Adjust depth
            if let Some(depth) = new_node.data.depth {
                let new_depth = (depth as i8 + self.offset).clamp(1, 6) as u8;
                new_node.data.depth = Some(new_depth);
            }

            Some(new_node)
        }
    }

    #[test]
    fn mut_visitor_adjusts_header_depth() {
        let arena = AstArena::new();

        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "Title"));
        let children = arena.alloc_slice_copy(&[*text]);
        let mut header = TxtNode::new_parent(NodeType::Header, Span::new(0, 7), children);
        header.data.depth = Some(1);
        let header = arena.alloc(header);

        let mut adjuster = HeaderDepthAdjuster {
            arena: &arena,
            offset: 1,
        };
        let result = walk_node_mut(&mut adjuster, header);

        assert!(result.is_some());
        let new_header = result.unwrap();
        assert_eq!(new_header.data.depth, Some(2));
    }

    #[test]
    fn mut_visitor_clamps_header_depth() {
        let arena = AstArena::new();

        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "Title"));
        let children = arena.alloc_slice_copy(&[*text]);
        let mut header = TxtNode::new_parent(NodeType::Header, Span::new(0, 7), children);
        header.data.depth = Some(6);
        let header = arena.alloc(header);

        let mut adjuster = HeaderDepthAdjuster {
            arena: &arena,
            offset: 2, // Would make it 8, but should clamp to 6
        };
        let result = walk_node_mut(&mut adjuster, header);

        assert!(result.is_some());
        let new_header = result.unwrap();
        assert_eq!(new_header.data.depth, Some(6));
    }
}
