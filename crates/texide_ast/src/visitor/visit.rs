//! Visitor trait for traversing TxtAST nodes.
//!
//! This module provides the `Visitor` trait for read-only AST traversal.
//! Each `visit_*` method has a default implementation that walks children,
//! allowing you to override only the node types you care about.
//!
//! # Example
//!
//! ```rust
//! use texide_ast::{TxtNode, NodeType, Span, AstArena};
//! use texide_ast::visitor::{Visitor, VisitResult, walk_node, walk_children};
//! use std::ops::ControlFlow;
//!
//! /// Collects all text content from an AST.
//! struct TextCollector<'a> {
//!     texts: Vec<&'a str>,
//! }
//!
//! impl<'a> Visitor<'a> for TextCollector<'a> {
//!     fn visit_str(&mut self, node: &TxtNode<'a>) -> VisitResult {
//!         if let Some(text) = node.value {
//!             self.texts.push(text);
//!         }
//!         ControlFlow::Continue(())
//!     }
//! }
//!
//! // Usage
//! let arena = AstArena::new();
//! let text_node = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
//! let children = arena.alloc_slice_copy(&[*text_node]);
//! let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 5), children);
//!
//! let mut collector = TextCollector { texts: Vec::new() };
//! walk_node(&mut collector, &doc);
//! assert_eq!(collector.texts, vec!["hello"]);
//! ```

use std::ops::ControlFlow;

use crate::TxtNode;

use super::walk::{walk_children, walk_node};

/// Result type for visitor methods to control traversal.
///
/// - `ControlFlow::Continue(())` - continue visiting children
/// - `ControlFlow::Break(())` - stop traversal early
pub type VisitResult = ControlFlow<()>;

/// Visitor trait for traversing TxtAST nodes without modification.
///
/// Each `visit_*` method has a default implementation that calls
/// `walk_children` to traverse child nodes. Override specific methods
/// to customize behavior for particular node types.
///
/// # Lifetime
///
/// The `'a` lifetime ties visited nodes to their arena allocator.
///
/// # Control Flow
///
/// Return `ControlFlow::Continue(())` to continue traversal, or
/// `ControlFlow::Break(())` to stop early. Use the `?` operator
/// for convenient propagation.
pub trait Visitor<'a>: Sized {
    /// Called before visiting any node. Can be used to set up context.
    #[inline]
    fn enter_node(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(())
    }

    /// Called after visiting a node and all its children.
    #[inline]
    fn exit_node(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(())
    }

    /// Visits any node by dispatching to the type-specific method.
    ///
    /// Override this if you need custom dispatch logic.
    #[inline]
    fn visit_node(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_node(self, node)
    }

    // === Block-level node visitors ===

    /// Visit a Document node.
    fn visit_document(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a Paragraph node.
    fn visit_paragraph(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a Header node.
    fn visit_header(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a BlockQuote node.
    fn visit_block_quote(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a List node.
    fn visit_list(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a ListItem node.
    fn visit_list_item(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a CodeBlock node.
    fn visit_code_block(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node, no children to walk
    }

    /// Visit a HorizontalRule node.
    fn visit_horizontal_rule(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }

    /// Visit an Html node.
    fn visit_html(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }

    // === Inline-level node visitors ===

    /// Visit a Str (text) node.
    fn visit_str(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Text leaf
    }

    /// Visit a Break node.
    fn visit_break(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }

    /// Visit an Emphasis node.
    fn visit_emphasis(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a Strong node.
    fn visit_strong(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a Delete node.
    fn visit_delete(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a Code (inline) node.
    fn visit_code(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Inline code leaf
    }

    /// Visit a Link node.
    fn visit_link(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit an Image node.
    fn visit_image(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }

    // === Reference node visitors ===

    /// Visit a LinkReference node.
    fn visit_link_reference(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit an ImageReference node.
    fn visit_image_reference(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }

    /// Visit a Definition node.
    fn visit_definition(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }

    // === Table node visitors (GFM) ===

    /// Visit a Table node.
    fn visit_table(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a TableRow node.
    fn visit_table_row(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a TableCell node.
    fn visit_table_cell(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    // === Footnote node visitors ===

    /// Visit a FootnoteDefinition node.
    fn visit_footnote_definition(&mut self, node: &TxtNode<'a>) -> VisitResult {
        walk_children(self, node)
    }

    /// Visit a FootnoteReference node.
    fn visit_footnote_reference(&mut self, _node: &TxtNode<'a>) -> VisitResult {
        ControlFlow::Continue(()) // Leaf node
    }
}
