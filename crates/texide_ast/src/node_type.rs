//! Node type definitions for TxtAST.
//!
//! These types are compatible with textlint's TxtAST specification.
//! See: https://textlint.github.io/docs/txtnode

use serde::{Deserialize, Serialize};

/// Node types for TxtAST.
///
/// These correspond to textlint's node types as defined in
/// `@textlint/ast-node-types`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub enum NodeType {
    // Document structure
    /// Root document node.
    Document,

    // Block elements
    /// Paragraph containing inline content.
    Paragraph,
    /// Header/Heading (H1-H6).
    Header,
    /// Block quote.
    BlockQuote,
    /// Ordered or unordered list.
    List,
    /// Item in a list.
    ListItem,
    /// Fenced or indented code block.
    CodeBlock,
    /// Horizontal rule / thematic break.
    HorizontalRule,
    /// Raw HTML block.
    Html,

    // Inline elements
    /// Plain text string.
    Str,
    /// Soft or hard line break.
    Break,
    /// Emphasis (italic).
    Emphasis,
    /// Strong emphasis (bold).
    Strong,
    /// Strikethrough text.
    Delete,
    /// Inline code.
    Code,
    /// Hyperlink.
    Link,
    /// Image.
    Image,

    // Reference elements (textlint v14.5.0+)
    /// Link reference.
    LinkReference,
    /// Image reference.
    ImageReference,
    /// Reference definition.
    Definition,

    // Extension elements (GFM, etc.)
    /// Table (GFM).
    Table,
    /// Table row (GFM).
    TableRow,
    /// Table cell (GFM).
    TableCell,
    /// Footnote definition.
    FootnoteDefinition,
    /// Footnote reference.
    FootnoteReference,
}

impl NodeType {
    /// Returns true if this node type is a block element.
    #[inline]
    pub const fn is_block(&self) -> bool {
        matches!(
            self,
            NodeType::Document
                | NodeType::Paragraph
                | NodeType::Header
                | NodeType::BlockQuote
                | NodeType::List
                | NodeType::ListItem
                | NodeType::CodeBlock
                | NodeType::HorizontalRule
                | NodeType::Html
                | NodeType::Table
                | NodeType::TableRow
                | NodeType::FootnoteDefinition
        )
    }

    /// Returns true if this node type is an inline element.
    #[inline]
    pub const fn is_inline(&self) -> bool {
        matches!(
            self,
            NodeType::Str
                | NodeType::Break
                | NodeType::Emphasis
                | NodeType::Strong
                | NodeType::Delete
                | NodeType::Code
                | NodeType::Link
                | NodeType::Image
                | NodeType::LinkReference
                | NodeType::ImageReference
                | NodeType::FootnoteReference
        )
    }

    /// Returns true if this node type can contain children.
    #[inline]
    pub const fn is_parent(&self) -> bool {
        matches!(
            self,
            NodeType::Document
                | NodeType::Paragraph
                | NodeType::Header
                | NodeType::BlockQuote
                | NodeType::List
                | NodeType::ListItem
                | NodeType::Emphasis
                | NodeType::Strong
                | NodeType::Delete
                | NodeType::Link
                | NodeType::Table
                | NodeType::TableRow
                | NodeType::TableCell
                | NodeType::FootnoteDefinition
        )
    }

    /// Returns true if this node type is a text node (has value).
    #[inline]
    pub const fn is_text(&self) -> bool {
        matches!(self, NodeType::Str | NodeType::Code | NodeType::CodeBlock)
    }
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use the same casing as serde serialization
        let name = match self {
            NodeType::Document => "Document",
            NodeType::Paragraph => "Paragraph",
            NodeType::Header => "Header",
            NodeType::BlockQuote => "BlockQuote",
            NodeType::List => "List",
            NodeType::ListItem => "ListItem",
            NodeType::CodeBlock => "CodeBlock",
            NodeType::HorizontalRule => "HorizontalRule",
            NodeType::Html => "Html",
            NodeType::Str => "Str",
            NodeType::Break => "Break",
            NodeType::Emphasis => "Emphasis",
            NodeType::Strong => "Strong",
            NodeType::Delete => "Delete",
            NodeType::Code => "Code",
            NodeType::Link => "Link",
            NodeType::Image => "Image",
            NodeType::LinkReference => "LinkReference",
            NodeType::ImageReference => "ImageReference",
            NodeType::Definition => "Definition",
            NodeType::Table => "Table",
            NodeType::TableRow => "TableRow",
            NodeType::TableCell => "TableCell",
            NodeType::FootnoteDefinition => "FootnoteDefinition",
            NodeType::FootnoteReference => "FootnoteReference",
        };
        write!(f, "{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_block() {
        assert!(NodeType::Paragraph.is_block());
        assert!(NodeType::Header.is_block());
        assert!(!NodeType::Str.is_block());
        assert!(!NodeType::Emphasis.is_block());
    }

    #[test]
    fn test_is_inline() {
        assert!(NodeType::Str.is_inline());
        assert!(NodeType::Emphasis.is_inline());
        assert!(!NodeType::Paragraph.is_inline());
        assert!(!NodeType::Document.is_inline());
    }

    #[test]
    fn test_is_parent() {
        assert!(NodeType::Document.is_parent());
        assert!(NodeType::Paragraph.is_parent());
        assert!(!NodeType::Str.is_parent());
        assert!(!NodeType::Code.is_parent());
    }

    #[test]
    fn test_display() {
        assert_eq!(NodeType::Document.to_string(), "Document");
        assert_eq!(NodeType::Str.to_string(), "Str");
        assert_eq!(NodeType::CodeBlock.to_string(), "CodeBlock");
    }

    #[test]
    fn test_display_all_types() {
        // Test display for all node types
        let types = vec![
            (NodeType::Document, "Document"),
            (NodeType::Paragraph, "Paragraph"),
            (NodeType::Header, "Header"),
            (NodeType::BlockQuote, "BlockQuote"),
            (NodeType::List, "List"),
            (NodeType::ListItem, "ListItem"),
            (NodeType::CodeBlock, "CodeBlock"),
            (NodeType::HorizontalRule, "HorizontalRule"),
            (NodeType::Html, "Html"),
            (NodeType::Str, "Str"),
            (NodeType::Break, "Break"),
            (NodeType::Emphasis, "Emphasis"),
            (NodeType::Strong, "Strong"),
            (NodeType::Delete, "Delete"),
            (NodeType::Code, "Code"),
            (NodeType::Link, "Link"),
            (NodeType::Image, "Image"),
            (NodeType::LinkReference, "LinkReference"),
            (NodeType::ImageReference, "ImageReference"),
            (NodeType::Definition, "Definition"),
            (NodeType::Table, "Table"),
            (NodeType::TableRow, "TableRow"),
            (NodeType::TableCell, "TableCell"),
            (NodeType::FootnoteDefinition, "FootnoteDefinition"),
            (NodeType::FootnoteReference, "FootnoteReference"),
        ];

        for (node_type, expected) in types {
            assert_eq!(node_type.to_string(), expected);
        }
    }

    #[test]
    fn test_is_text() {
        assert!(NodeType::Str.is_text());
        assert!(NodeType::Code.is_text());
        assert!(NodeType::CodeBlock.is_text());
        assert!(!NodeType::Paragraph.is_text());
        assert!(!NodeType::Link.is_text());
    }

    #[test]
    fn test_block_elements_comprehensive() {
        let block_types = vec![
            NodeType::Document,
            NodeType::Paragraph,
            NodeType::Header,
            NodeType::BlockQuote,
            NodeType::List,
            NodeType::ListItem,
            NodeType::CodeBlock,
            NodeType::HorizontalRule,
            NodeType::Html,
            NodeType::Table,
            NodeType::TableRow,
            NodeType::FootnoteDefinition,
        ];

        for node_type in block_types {
            assert!(node_type.is_block(), "{:?} should be block", node_type);
        }
    }

    #[test]
    fn test_inline_elements_comprehensive() {
        let inline_types = vec![
            NodeType::Str,
            NodeType::Break,
            NodeType::Emphasis,
            NodeType::Strong,
            NodeType::Delete,
            NodeType::Code,
            NodeType::Link,
            NodeType::Image,
            NodeType::LinkReference,
            NodeType::ImageReference,
            NodeType::FootnoteReference,
        ];

        for node_type in inline_types {
            assert!(node_type.is_inline(), "{:?} should be inline", node_type);
        }
    }

    #[test]
    fn test_parent_elements_comprehensive() {
        let parent_types = vec![
            NodeType::Document,
            NodeType::Paragraph,
            NodeType::Header,
            NodeType::BlockQuote,
            NodeType::List,
            NodeType::ListItem,
            NodeType::Emphasis,
            NodeType::Strong,
            NodeType::Delete,
            NodeType::Link,
            NodeType::Table,
            NodeType::TableRow,
            NodeType::TableCell,
            NodeType::FootnoteDefinition,
        ];

        for node_type in parent_types {
            assert!(node_type.is_parent(), "{:?} should be parent", node_type);
        }
    }

    #[test]
    fn test_non_parent_elements() {
        let non_parent_types = vec![
            NodeType::Str,
            NodeType::Break,
            NodeType::Code,
            NodeType::CodeBlock,
            NodeType::HorizontalRule,
            NodeType::Html,
            NodeType::Image,
            NodeType::Definition,
            NodeType::LinkReference,
            NodeType::ImageReference,
            NodeType::FootnoteReference,
        ];

        for node_type in non_parent_types {
            assert!(
                !node_type.is_parent(),
                "{:?} should not be parent",
                node_type
            );
        }
    }

    #[test]
    fn test_node_type_equality() {
        assert_eq!(NodeType::Document, NodeType::Document);
        assert_ne!(NodeType::Document, NodeType::Paragraph);
    }

    #[test]
    fn test_node_type_clone() {
        let original = NodeType::Header;
        let cloned = original;
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_node_type_debug() {
        let debug_str = format!("{:?}", NodeType::Paragraph);
        assert_eq!(debug_str, "Paragraph");
    }

    #[test]
    fn test_node_type_serialization() {
        let node_type = NodeType::Paragraph;
        let json = serde_json::to_string(&node_type).unwrap();
        assert_eq!(json, "\"Paragraph\"");
    }

    #[test]
    fn test_node_type_deserialization() {
        let json = "\"Header\"";
        let node_type: NodeType = serde_json::from_str(json).unwrap();
        assert_eq!(node_type, NodeType::Header);
    }

    #[test]
    fn test_table_cell_is_parent_not_block() {
        // TableCell is special: it's a parent but not a block element
        assert!(NodeType::TableCell.is_parent());
        assert!(!NodeType::TableCell.is_block());
    }

    #[test]
    fn test_link_is_both_inline_and_parent() {
        // Link can contain children (like text) and is also an inline element
        assert!(NodeType::Link.is_inline());
        assert!(NodeType::Link.is_parent());
    }

    #[test]
    fn test_definition_is_not_block_or_inline() {
        // Definition is a special reference element
        assert!(!NodeType::Definition.is_block());
        assert!(!NodeType::Definition.is_inline());
    }
}
