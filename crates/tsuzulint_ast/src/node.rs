//! TxtNode definition.
//!
//! The core AST node type used throughout TsuzuLint.

use serde::Serialize;

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

#[derive(Debug, Clone, Copy, Default)]
pub enum NodeData<'a> {
    #[default]
    None,
    Header(u8),
    List(bool),
    CodeBlock(Option<&'a str>),
    Link(LinkData<'a>),
    Reference(ReferenceData<'a>),
    Definition(DefinitionData<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct LinkData<'a> {
    pub url: &'a str,
    pub title: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct ReferenceData<'a> {
    pub identifier: &'a str,
    pub label: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct DefinitionData<'a> {
    pub identifier: &'a str,
    pub url: &'a str,
    pub title: Option<&'a str>,
    pub label: Option<&'a str>,
}

impl<'a> Serialize for TxtNode<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut len = 2; // type, range
        if self.node_type.is_parent() || !self.children.is_empty() {
            len += 1;
        }
        if self.value.is_some() {
            len += 1;
        }
        len += self.data.present_field_count();

        let mut state = serializer.serialize_struct("TxtNode", len)?;

        state.serialize_field("type", &self.node_type)?;
        state.serialize_field("range", &[self.span.start, self.span.end])?;

        if self.node_type.is_parent() || !self.children.is_empty() {
            state.serialize_field("children", &self.children)?;
        }

        if let Some(value) = &self.value {
            state.serialize_field("value", value)?;
        }

        self.data.serialize_fields(&mut state)?;

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
    /// Returns the number of present (non-None) fields for serialization.
    fn present_field_count(&self) -> usize {
        match self {
            NodeData::None => 0,
            NodeData::Header(_) => 1,
            NodeData::List(_) => 1,
            NodeData::CodeBlock(lang) => {
                if lang.is_some() {
                    1
                } else {
                    0
                }
            }
            NodeData::Link(link_data) => {
                if link_data.title.is_some() {
                    2
                } else {
                    1
                }
            }
            NodeData::Reference(ref_data) => {
                if ref_data.label.is_some() {
                    2
                } else {
                    1
                }
            }
            NodeData::Definition(def_data) => {
                let mut count = 2;
                if def_data.title.is_some() {
                    count += 1;
                }
                if def_data.label.is_some() {
                    count += 1;
                }
                count
            }
        }
    }

    /// Serializes present fields into the given struct serializer state.
    fn serialize_fields<S: serde::ser::SerializeStruct>(
        &self,
        state: &mut S,
    ) -> Result<(), S::Error> {
        match self {
            NodeData::None => {}
            NodeData::Header(depth) => {
                state.serialize_field("depth", depth)?;
            }
            NodeData::List(ordered) => {
                state.serialize_field("ordered", ordered)?;
            }
            NodeData::CodeBlock(lang) => {
                if let Some(l) = lang {
                    state.serialize_field("lang", l)?;
                }
            }
            NodeData::Link(link_data) => {
                state.serialize_field("url", link_data.url)?;
                if let Some(title) = link_data.title {
                    state.serialize_field("title", title)?;
                }
            }
            NodeData::Reference(ref_data) => {
                state.serialize_field("identifier", ref_data.identifier)?;
                if let Some(label) = ref_data.label {
                    state.serialize_field("label", label)?;
                }
            }
            NodeData::Definition(def_data) => {
                state.serialize_field("identifier", def_data.identifier)?;
                state.serialize_field("url", def_data.url)?;
                if let Some(title) = def_data.title {
                    state.serialize_field("title", title)?;
                }
                if let Some(label) = def_data.label {
                    state.serialize_field("label", label)?;
                }
            }
        }
        Ok(())
    }

    /// Creates new empty node data.
    #[inline]
    pub const fn new() -> Self {
        Self::None
    }

    /// Creates node data for a header.
    #[inline]
    pub const fn header(depth: u8) -> Self {
        Self::Header(depth)
    }

    /// Creates node data for a link.
    #[inline]
    pub const fn link(url: &'a str, title: Option<&'a str>) -> Self {
        Self::Link(LinkData { url, title })
    }

    /// Creates node data for a code block.
    #[inline]
    pub const fn code_block(lang: Option<&'a str>) -> Self {
        Self::CodeBlock(lang)
    }

    /// Creates node data for a list.
    #[inline]
    pub const fn list(ordered: bool) -> Self {
        Self::List(ordered)
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
        assert!(matches!(data, NodeData::Header(2)));
    }

    #[test]
    fn test_node_data_link() {
        let data = NodeData::link("https://example.com", Some("Example"));
        match data {
            NodeData::Link(link_data) => {
                assert_eq!(link_data.url, "https://example.com");
                assert_eq!(link_data.title, Some("Example"));
            }
            _ => panic!("Expected Link variant"),
        }
    }

    #[test]
    fn test_node_data_link_without_title() {
        let data = NodeData::link("https://example.com", None);
        match data {
            NodeData::Link(link_data) => {
                assert_eq!(link_data.url, "https://example.com");
                assert!(link_data.title.is_none());
            }
            _ => panic!("Expected Link variant"),
        }
    }

    #[test]
    fn test_node_data_code_block() {
        let data = NodeData::code_block(Some("rust"));
        assert!(matches!(data, NodeData::CodeBlock(Some("rust"))));
    }

    #[test]
    fn test_node_data_code_block_without_lang() {
        let data = NodeData::code_block(None);
        assert!(matches!(data, NodeData::CodeBlock(None)));
    }

    #[test]
    fn test_node_data_list_ordered() {
        let data = NodeData::list(true);
        assert!(matches!(data, NodeData::List(true)));
    }

    #[test]
    fn test_node_data_list_unordered() {
        let data = NodeData::list(false);
        assert!(matches!(data, NodeData::List(false)));
    }

    #[test]
    fn test_node_data_new_empty() {
        let data = NodeData::new();
        assert!(matches!(data, NodeData::None));
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
            assert!(matches!(data, NodeData::Header(d) if d == depth));
        }
    }

    #[test]
    fn test_node_data_default() {
        let data = NodeData::default();
        assert!(matches!(data, NodeData::None));
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
        assert!(matches!(node.data, NodeData::CodeBlock(Some("rust"))));
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

        let json = serde_json::to_value(node).unwrap();

        assert_eq!(json["type"], "Header");
        assert_eq!(json["depth"], 2);
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

    #[test]
    fn test_serialization_definition_all_fields() {
        let mut node = TxtNode::new_leaf(NodeType::Definition, Span::new(0, 10));
        node.data = NodeData::Definition(DefinitionData {
            identifier: "id",
            url: "http://url",
            title: Some("Title"),
            label: Some("lbl"),
        });

        let json = serde_json::to_value(node).unwrap();
        let obj = json.as_object().unwrap();

        // Expected fields: type, range, identifier, url, title, label
        // Total = 6
        assert_eq!(obj.len(), 6);

        assert_eq!(obj["identifier"], "id");
        assert_eq!(obj["url"], "http://url");
        assert_eq!(obj["title"], "Title");
        assert_eq!(obj["label"], "lbl");
    }

    #[test]
    fn test_serialization_leaf_no_children() {
        let node = TxtNode::new_leaf(NodeType::HorizontalRule, Span::new(0, 5));
        let json = serde_json::to_value(node).unwrap();
        let obj = json.as_object().unwrap();

        // Expected fields: type, range
        // Total = 2
        assert_eq!(obj.len(), 2);
        assert!(!obj.contains_key("children"));
        assert!(!obj.contains_key("value"));
    }

    #[test]
    fn test_nodedata_size_optimization() {
        use std::mem::size_of;

        let old_size = 7 * size_of::<Option<&str>>();
        let new_size = size_of::<NodeData>();

        assert!(
            new_size < old_size,
            "NodeData should be smaller: was {} bytes, now {} bytes",
            old_size,
            new_size
        );
    }
}
