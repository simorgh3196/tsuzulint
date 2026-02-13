//! TxtNode definition.
//!
//! The core AST node type used throughout TsuzuLint.

use serde::{Serialize, ser::SerializeStruct};

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
/// use tsuzulint_ast::{AstArena, TxtNode, NodeType, Span};
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

impl<'a> Serialize for TxtNode<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Estimate field count: type + range = 2, children? value? data fields?
        let mut state = serializer.serialize_struct("TxtNode", 5)?;

        // 1. Type
        // Serialize as string (Display implementation of NodeType matches PascalCase)
        state.serialize_field("type", &self.node_type)?;

        // 2. Range
        // TextLint uses [start, end] array for range
        state.serialize_field("range", &[self.span.start, self.span.end])?;

        // 3. Children
        // Include children if present OR if it's a parent type (even if empty)
        // This matches tsuzulint_wasm behavior
        if self.node_type.is_parent() || !self.children.is_empty() {
            state.serialize_field("children", &self.children)?;
        }

        // 4. Value
        if let Some(val) = self.value {
            state.serialize_field("value", val)?;
        }

        // 5. Data fields (Flattened)
        if let Some(url) = self.data.url {
            state.serialize_field("url", url)?;
        }
        if let Some(title) = self.data.title {
            state.serialize_field("title", title)?;
        }
        if let Some(depth) = self.data.depth {
            state.serialize_field("depth", &depth)?;
        }
        if let Some(ordered) = self.data.ordered {
            state.serialize_field("ordered", &ordered)?;
        }
        if let Some(lang) = self.data.lang {
            state.serialize_field("lang", lang)?;
        }
        if let Some(identifier) = self.data.identifier {
            state.serialize_field("identifier", identifier)?;
        }
        if let Some(label) = self.data.label {
            state.serialize_field("label", label)?;
        }

        state.end()
    }
}

impl<'a> TxtNode<'a> {
    /// Creates a new parent node with children.
    #[inline]
    pub const fn new_parent(node_type: NodeType, span: Span, children: &'a [TxtNode<'a>]) -> Self {
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

    #[test]
    fn test_node_data_link_without_title() {
        let data = NodeData::link("https://example.com", None);
        assert_eq!(data.url, Some("https://example.com"));
        assert!(data.title.is_none());
    }

    #[test]
    fn test_node_data_code_block() {
        let data = NodeData::code_block(Some("rust"));
        assert_eq!(data.lang, Some("rust"));
    }

    #[test]
    fn test_node_data_code_block_without_lang() {
        let data = NodeData::code_block(None);
        assert!(data.lang.is_none());
    }

    #[test]
    fn test_node_data_list_ordered() {
        let data = NodeData::list(true);
        assert_eq!(data.ordered, Some(true));
    }

    #[test]
    fn test_node_data_list_unordered() {
        let data = NodeData::list(false);
        assert_eq!(data.ordered, Some(false));
    }

    #[test]
    fn test_node_data_new_empty() {
        let data = NodeData::new();
        assert!(data.url.is_none());
        assert!(data.title.is_none());
        assert!(data.depth.is_none());
        assert!(data.ordered.is_none());
        assert!(data.lang.is_none());
        assert!(data.identifier.is_none());
        assert!(data.label.is_none());
    }

    #[test]
    fn test_new_leaf() {
        let node = TxtNode::new_leaf(NodeType::HorizontalRule, Span::new(0, 3));

        assert_eq!(node.node_type, NodeType::HorizontalRule);
        assert!(!node.is_text());
        assert!(!node.has_children());
        assert!(node.value.is_none());
    }

    #[test]
    fn test_node_span() {
        let node = TxtNode::new_leaf(NodeType::Break, Span::new(10, 20));

        assert_eq!(node.span.start, 10);
        assert_eq!(node.span.end, 20);
    }

    #[test]
    fn test_node_with_multiple_children() {
        let arena = AstArena::new();

        let child1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let child2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 11), "world"));
        let child3 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(12, 13), "!"));

        let children = arena.alloc_slice_copy(&[*child1, *child2, *child3]);
        let node = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 13), children);

        assert_eq!(node.children.len(), 3);
        assert_eq!(node.children[0].value, Some("hello"));
        assert_eq!(node.children[1].value, Some("world"));
        assert_eq!(node.children[2].value, Some("!"));
    }

    #[test]
    fn test_nested_parent_nodes() {
        let arena = AstArena::new();

        // Create a text node
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 4), "text"));
        let text_children = arena.alloc_slice_copy(&[*text]);

        // Create an emphasis containing the text
        let emphasis = arena.alloc(TxtNode::new_parent(
            NodeType::Emphasis,
            Span::new(0, 6),
            text_children,
        ));
        let emphasis_children = arena.alloc_slice_copy(&[*emphasis]);

        // Create a paragraph containing the emphasis
        let paragraph =
            TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 6), emphasis_children);

        assert_eq!(paragraph.node_type, NodeType::Paragraph);
        assert_eq!(paragraph.children[0].node_type, NodeType::Emphasis);
        assert_eq!(paragraph.children[0].children[0].value, Some("text"));
    }

    #[test]
    fn test_node_text_method() {
        let text_node = TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello");
        let parent_node = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 5), &[]);

        assert_eq!(text_node.text(), Some("hello"));
        assert_eq!(parent_node.text(), None);
    }

    #[test]
    fn test_header_depth_values() {
        for depth in 1u8..=6 {
            let data = NodeData::header(depth);
            assert_eq!(data.depth, Some(depth));
        }
    }

    #[test]
    fn test_node_data_default() {
        let data = NodeData::default();
        assert!(data.url.is_none());
        assert!(data.title.is_none());
        assert!(data.depth.is_none());
    }

    #[test]
    fn test_empty_children_slice() {
        let node = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 0), &[]);

        assert!(node.children.is_empty());
        assert!(!node.has_children());
    }

    #[test]
    fn test_code_node_is_text() {
        let node = TxtNode::new_text(NodeType::Code, Span::new(0, 10), "console.log");

        assert!(node.is_text());
        assert_eq!(node.value, Some("console.log"));
    }

    #[test]
    fn test_code_block_node() {
        let arena = AstArena::new();
        let code = "fn main() {}";
        let mut node =
            TxtNode::new_text(NodeType::CodeBlock, Span::new(0, 12), arena.alloc_str(code));
        node.data = NodeData::code_block(Some("rust"));

        assert_eq!(node.node_type, NodeType::CodeBlock);
        assert_eq!(node.data.lang, Some("rust"));
        assert_eq!(node.value, Some(code));
    }

    #[test]
    fn test_serialization_basic() {
        let node = TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello");
        let json = serde_json::to_value(node).unwrap();

        assert_eq!(json["type"], "Str");
        assert_eq!(json["range"][0], 0);
        assert_eq!(json["range"][1], 5);
        assert_eq!(json["value"], "hello");
        // No children for leaf text node
        assert!(json.get("children").is_none());
    }

    #[test]
    fn test_serialization_parent() {
        let arena = AstArena::new();
        let child = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "hello"));
        let children = arena.alloc_slice_copy(&[*child]);
        let node = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 5), children);

        let json = serde_json::to_value(node).unwrap();

        assert_eq!(json["type"], "Paragraph");
        assert_eq!(json["range"][0], 0);
        assert_eq!(json["range"][1], 5);
        assert!(json["children"].is_array());
        assert_eq!(json["children"].as_array().unwrap().len(), 1);
        assert_eq!(json["children"][0]["type"], "Str");
    }

    #[test]
    fn test_serialization_flattened_data() {
        let mut node = TxtNode::new_parent(NodeType::Header, Span::new(0, 10), &[]);
        node.data = NodeData::header(2);
        node.data.url = Some("https://example.com");

        let json = serde_json::to_value(node).unwrap();

        assert_eq!(json["type"], "Header");
        assert_eq!(json["depth"], 2);
        assert_eq!(json["url"], "https://example.com");
    }

    #[test]
    fn test_serialization_empty_parent() {
        // Parent node with no children should still have "children": []
        let node = TxtNode::new_parent(NodeType::Paragraph, Span::new(0, 0), &[]);
        let json = serde_json::to_value(node).unwrap();

        assert_eq!(json["type"], "Paragraph");
        assert!(json["children"].is_array());
        assert!(json["children"].as_array().unwrap().is_empty());
    }
}
