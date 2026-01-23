//! TxtNode definition.
//!
//! The core AST node type used throughout Texide.

use crate::{NodeType, Span};

/// A node in the TxtAST.
///
/// TxtNode represents a node in the abstract syntax tree for natural language
/// text. It is designed to be allocated in an arena for efficiency.
///
/// # Lifetime
///
/// The `'a` lifetime parameter ties this node to its arena allocator,
/// ensuring that all child references remain valid.
///
/// # Example
///
/// ```rust
/// use texide_ast::{AstArena, TxtNode, NodeType, Span};
///
/// let arena = AstArena::new();
///
/// // Create a text node
/// let text_node = arena.alloc(TxtNode::new_text(
///     NodeType::Str,
///     Span::new(0, 5),
///     "Hello",
/// ));
///
/// // Create a paragraph containing the text
/// let children = arena.alloc_slice_copy(&[*text_node]);
/// let paragraph = TxtNode::new_parent(
///     NodeType::Paragraph,
///     Span::new(0, 5),
///     children,
/// );
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TxtNode<'a> {
    /// The type of this node.
    pub node_type: NodeType,

    /// Byte span in the source text.
    pub span: Span,

    /// Child nodes (for parent nodes).
    pub children: &'a [TxtNode<'a>],

    /// Text value (for text nodes like Str, Code, CodeBlock).
    pub value: Option<&'a str>,

    /// Additional node-specific data.
    pub data: NodeData<'a>,
}

/// Additional data specific to certain node types.
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeData<'a> {
    /// URL for Link/Image nodes.
    pub url: Option<&'a str>,

    /// Title for Link/Image nodes.
    pub title: Option<&'a str>,

    /// Depth for Header nodes (1-6).
    pub depth: Option<u8>,

    /// Whether list is ordered.
    pub ordered: Option<bool>,

    /// Language for CodeBlock nodes.
    pub lang: Option<&'a str>,

    /// Identifier for reference nodes.
    pub identifier: Option<&'a str>,

    /// Label for reference nodes.
    pub label: Option<&'a str>,
}

impl<'a> TxtNode<'a> {
    /// Creates a new parent node with children.
    #[inline]
    pub const fn new_parent(
        node_type: NodeType,
        span: Span,
        children: &'a [TxtNode<'a>],
    ) -> Self {
        Self {
            node_type,
            span,
            children,
            value: None,
            data: NodeData::new(),
        }
    }

    /// Creates a new text node with a value.
    #[inline]
    pub const fn new_text(node_type: NodeType, span: Span, value: &'a str) -> Self {
        Self {
            node_type,
            span,
            children: &[],
            value: Some(value),
            data: NodeData::new(),
        }
    }

    /// Creates a new leaf node (no children, no value).
    #[inline]
    pub const fn new_leaf(node_type: NodeType, span: Span) -> Self {
        Self {
            node_type,
            span,
            children: &[],
            value: None,
            data: NodeData::new(),
        }
    }

    /// Returns true if this node has children.
    #[inline]
    pub const fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns true if this node is a text node.
    #[inline]
    pub const fn is_text(&self) -> bool {
        self.value.is_some()
    }

    /// Returns the raw text content of this node.
    ///
    /// For text nodes, returns the value.
    /// For parent nodes, this returns None (use a visitor to collect text).
    #[inline]
    pub const fn text(&self) -> Option<&'a str> {
        self.value
    }
}

impl<'a> NodeData<'a> {
    /// Creates new empty node data.
    #[inline]
    pub const fn new() -> Self {
        Self {
            url: None,
            title: None,
            depth: None,
            ordered: None,
            lang: None,
            identifier: None,
            label: None,
        }
    }

    /// Creates node data for a header.
    #[inline]
    pub const fn header(depth: u8) -> Self {
        Self {
            depth: Some(depth),
            ..Self::new()
        }
    }

    /// Creates node data for a link.
    #[inline]
    pub const fn link(url: &'a str, title: Option<&'a str>) -> Self {
        Self {
            url: Some(url),
            title,
            ..Self::new()
        }
    }

    /// Creates node data for a code block.
    #[inline]
    pub const fn code_block(lang: Option<&'a str>) -> Self {
        Self {
            lang,
            ..Self::new()
        }
    }

    /// Creates node data for a list.
    #[inline]
    pub const fn list(ordered: bool) -> Self {
        Self {
            ordered: Some(ordered),
            ..Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AstArena;

    #[test]
    fn test_new_parent() {
        let arena = AstArena::new();
        let child = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let children = arena.alloc_slice_copy(&[*child]);
        let node = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 5), children);

        assert_eq!(node.node_type, NodeType::Paragraph);
        assert!(node.has_children());
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn test_new_text() {
        let node = TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello");

        assert_eq!(node.node_type, NodeType::Str);
        assert!(node.is_text());
        assert_eq!(node.text(), Some("hello"));
        assert!(!node.has_children());
    }

    #[test]
    fn test_node_data_header() {
        let data = NodeData::header(2);
        assert_eq!(data.depth, Some(2));
    }

    #[test]
    fn test_node_data_link() {
        let data = NodeData::link("https://example.com", Some("Example"));
        assert_eq!(data.url, Some("https://example.com"));
        assert_eq!(data.title, Some("Example"));
    }
}
