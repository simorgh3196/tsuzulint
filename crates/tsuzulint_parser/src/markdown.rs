//! Markdown parser using markdown-rs (wooorm/markdown-rs).
//!
//! This parser converts Markdown to TxtAST using the `markdown` crate,
//! which provides mdast-compatible AST output.

use markdown::{ParseOptions, to_mdast};
use tsuzulint_ast::{AstArena, NodeData, NodeType, Span, TxtNode};

use crate::{ParseError, Parser};

/// Markdown parser implementation.
///
/// Uses `markdown-rs` for parsing, which supports:
/// - CommonMark
/// - GFM (GitHub Flavored Markdown)
/// - MDX (optional)
/// - Math (optional)
/// - Frontmatter (optional)
pub struct MarkdownParser;

impl MarkdownParser {
    /// Creates a new Markdown parser with default options.
    pub fn new() -> Self {
        Self
    }

    /// Gets default parse options (GFM).
    fn default_options() -> ParseOptions {
        ParseOptions::gfm()
    }

    /// Converts an mdast node to TxtNode.
    fn convert_node<'a>(
        &self,
        arena: &'a AstArena,
        node: &markdown::mdast::Node,
        source: &str,
    ) -> TxtNode<'a> {
        use markdown::mdast::Node;

        match node {
            Node::Root(root) => {
                let children = self.convert_children(arena, &root.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::Document, span, children)
            }

            Node::Paragraph(para) => {
                let children = self.convert_children(arena, &para.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::Paragraph, span, children)
            }

            Node::Heading(heading) => {
                let children = self.convert_children(arena, &heading.children, source);
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_parent(NodeType::Header, span, children);
                node.data = NodeData::header(heading.depth);
                node
            }

            Node::Text(text) => {
                let span = self.node_span(node, source);
                let value = arena.alloc_str(&text.value);
                TxtNode::new_text(NodeType::Str, span, value)
            }

            Node::Emphasis(em) => {
                let children = self.convert_children(arena, &em.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::Emphasis, span, children)
            }

            Node::Strong(strong) => {
                let children = self.convert_children(arena, &strong.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::Strong, span, children)
            }

            Node::InlineCode(code) => {
                let span = self.node_span(node, source);
                let value = arena.alloc_str(&code.value);
                TxtNode::new_text(NodeType::Code, span, value)
            }

            Node::Code(code) => {
                let span = self.node_span(node, source);
                let value = arena.alloc_str(&code.value);
                let mut node = TxtNode::new_text(NodeType::CodeBlock, span, value);
                if let Some(lang) = &code.lang {
                    node.data = NodeData::code_block(Some(arena.alloc_str(lang)));
                }
                node
            }

            Node::Link(link) => {
                let children = self.convert_children(arena, &link.children, source);
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_parent(NodeType::Link, span, children);
                let url = arena.alloc_str(&link.url);
                let title = link.title.as_ref().map(|t| arena.alloc_str(t));
                node.data = NodeData::link(url, title);
                node
            }

            Node::Image(image) => {
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_leaf(NodeType::Image, span);
                let url = arena.alloc_str(&image.url);
                let title = image.title.as_ref().map(|t| arena.alloc_str(t));
                node.data = NodeData::link(url, title);
                node
            }

            Node::List(list) => {
                let children = self.convert_children(arena, &list.children, source);
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_parent(NodeType::List, span, children);
                node.data = NodeData::list(list.ordered);
                node
            }

            Node::ListItem(item) => {
                let children = self.convert_children(arena, &item.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::ListItem, span, children)
            }

            Node::Blockquote(quote) => {
                let children = self.convert_children(arena, &quote.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::BlockQuote, span, children)
            }

            Node::ThematicBreak(_) => {
                let span = self.node_span(node, source);
                TxtNode::new_leaf(NodeType::HorizontalRule, span)
            }

            Node::Break(_) => {
                let span = self.node_span(node, source);
                TxtNode::new_leaf(NodeType::Break, span)
            }

            Node::Html(html) => {
                let span = self.node_span(node, source);
                let value = arena.alloc_str(&html.value);
                TxtNode::new_text(NodeType::Html, span, value)
            }

            Node::Delete(del) => {
                let children = self.convert_children(arena, &del.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::Delete, span, children)
            }

            // Table support (GFM)
            Node::Table(table) => {
                let children = self.convert_children(arena, &table.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::Table, span, children)
            }

            Node::TableRow(row) => {
                let children = self.convert_children(arena, &row.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::TableRow, span, children)
            }

            Node::TableCell(cell) => {
                let children = self.convert_children(arena, &cell.children, source);
                let span = self.node_span(node, source);
                TxtNode::new_parent(NodeType::TableCell, span, children)
            }

            // Footnotes (GFM)
            Node::FootnoteDefinition(def) => {
                let children = self.convert_children(arena, &def.children, source);
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_parent(NodeType::FootnoteDefinition, span, children);
                node.data.identifier = Some(arena.alloc_str(&def.identifier));
                if let Some(label) = &def.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            Node::FootnoteReference(ref_node) => {
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_leaf(NodeType::FootnoteReference, span);
                node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
                if let Some(label) = &ref_node.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            // Reference nodes
            Node::LinkReference(ref_node) => {
                let children = self.convert_children(arena, &ref_node.children, source);
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_parent(NodeType::LinkReference, span, children);
                node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
                if let Some(label) = &ref_node.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            Node::ImageReference(ref_node) => {
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_leaf(NodeType::ImageReference, span);
                node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
                if let Some(label) = &ref_node.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            Node::Definition(def) => {
                let span = self.node_span(node, source);
                let mut node = TxtNode::new_leaf(NodeType::Definition, span);
                node.data.identifier = Some(arena.alloc_str(&def.identifier));
                node.data.url = Some(arena.alloc_str(&def.url));
                if let Some(title) = &def.title {
                    node.data.title = Some(arena.alloc_str(title));
                }
                if let Some(label) = &def.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            // Fallback for unsupported nodes
            _ => {
                let span = self.node_span(node, source);
                TxtNode::new_leaf(NodeType::Html, span)
            }
        }
    }

    /// Converts a list of mdast children to TxtNode slice.
    fn convert_children<'a>(
        &self,
        arena: &'a AstArena,
        children: &[markdown::mdast::Node],
        source: &str,
    ) -> &'a [TxtNode<'a>] {
        let nodes: Vec<TxtNode<'a>> = children
            .iter()
            .map(|child| self.convert_node(arena, child, source))
            .collect();

        arena.alloc_slice_clone(&nodes)
    }

    /// Gets the span for an mdast node.
    fn node_span(&self, node: &markdown::mdast::Node, _source: &str) -> Span {
        if let Some(pos) = node.position() {
            Span::new(pos.start.offset as u32, pos.end.offset as u32)
        } else {
            Span::new(0, 0)
        }
    }
}

impl Default for MarkdownParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for MarkdownParser {
    fn name(&self) -> &str {
        "markdown"
    }

    fn extensions(&self) -> &[&str] {
        &["md", "markdown", "mdown", "mkdn", "mkd"]
    }

    fn parse<'a>(&self, arena: &'a AstArena, source: &str) -> Result<TxtNode<'a>, ParseError> {
        let options = Self::default_options();
        let mdast =
            to_mdast(source, &options).map_err(|e| ParseError::invalid_source(e.to_string()))?;

        Ok(self.convert_node(arena, &mdast, source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_markdown() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "# Hello\n\nThis is a paragraph.";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.node_type, NodeType::Document);
        assert!(ast.has_children());
    }

    #[test]
    fn test_parse_heading() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "# Level 1\n\n## Level 2";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.children.len(), 2);
        assert_eq!(ast.children[0].node_type, NodeType::Header);
        assert_eq!(ast.children[0].data.depth, Some(1));
        assert_eq!(ast.children[1].node_type, NodeType::Header);
        assert_eq!(ast.children[1].data.depth, Some(2));
    }

    #[test]
    fn test_parse_link() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "[Example](https://example.com)";

        let ast = parser.parse(&arena, source).unwrap();

        // Document > Paragraph > Link
        let paragraph = &ast.children[0];
        let link = &paragraph.children[0];

        assert_eq!(link.node_type, NodeType::Link);
        assert_eq!(link.data.url, Some("https://example.com"));
    }

    #[test]
    fn test_extensions() {
        let parser = MarkdownParser::new();

        assert!(parser.can_parse("md"));
        assert!(parser.can_parse("markdown"));
        assert!(parser.can_parse("MD"));
        assert!(!parser.can_parse("txt"));
    }

    #[test]
    fn test_parse_empty_document() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.node_type, NodeType::Document);
        assert!(ast.children.is_empty());
    }

    #[test]
    fn test_parse_emphasis() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "*italic* and **bold**";

        let ast = parser.parse(&arena, source).unwrap();

        // Document > Paragraph > [Emphasis, Str, Strong]
        let paragraph = &ast.children[0];
        assert_eq!(paragraph.node_type, NodeType::Paragraph);

        let has_emphasis = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Emphasis);
        let has_strong = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Strong);

        assert!(has_emphasis);
        assert!(has_strong);
    }

    #[test]
    fn test_parse_code_block() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "```rust\nfn main() {}\n```";

        let ast = parser.parse(&arena, source).unwrap();

        let code_block = &ast.children[0];
        assert_eq!(code_block.node_type, NodeType::CodeBlock);
        assert_eq!(code_block.data.lang, Some("rust"));
        assert!(code_block.value.is_some());
    }

    #[test]
    fn test_parse_code_block_no_language() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "```\nplain code\n```";

        let ast = parser.parse(&arena, source).unwrap();

        let code_block = &ast.children[0];
        assert_eq!(code_block.node_type, NodeType::CodeBlock);
        assert!(code_block.data.lang.is_none());
    }

    #[test]
    fn test_parse_inline_code() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Use `code` here";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_code = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Code);

        assert!(has_code);
    }

    #[test]
    fn test_parse_blockquote() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "> This is a quote";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.children[0].node_type, NodeType::BlockQuote);
    }

    #[test]
    fn test_parse_unordered_list() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "- Item 1\n- Item 2\n- Item 3";

        let ast = parser.parse(&arena, source).unwrap();

        let list = &ast.children[0];
        assert_eq!(list.node_type, NodeType::List);
        assert_eq!(list.data.ordered, Some(false));
        assert_eq!(list.children.len(), 3);

        for item in list.children.iter() {
            assert_eq!(item.node_type, NodeType::ListItem);
        }
    }

    #[test]
    fn test_parse_ordered_list() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "1. First\n2. Second\n3. Third";

        let ast = parser.parse(&arena, source).unwrap();

        let list = &ast.children[0];
        assert_eq!(list.node_type, NodeType::List);
        assert_eq!(list.data.ordered, Some(true));
    }

    #[test]
    fn test_parse_horizontal_rule() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "---";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.children[0].node_type, NodeType::HorizontalRule);
    }

    #[test]
    fn test_parse_image() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "![Alt text](image.png \"Title\")";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let image = &paragraph.children[0];

        assert_eq!(image.node_type, NodeType::Image);
        assert_eq!(image.data.url, Some("image.png"));
        assert_eq!(image.data.title, Some("Title"));
    }

    #[test]
    fn test_parse_link_with_title() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "[Link](https://example.com \"Example Title\")";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let link = &paragraph.children[0];

        assert_eq!(link.node_type, NodeType::Link);
        assert_eq!(link.data.url, Some("https://example.com"));
        assert_eq!(link.data.title, Some("Example Title"));
    }

    #[test]
    fn test_parse_strikethrough() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "~~deleted text~~";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_delete = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Delete);

        assert!(has_delete);
    }

    #[test]
    fn test_parse_table() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "| Header 1 | Header 2 |\n|----------|----------|\n| Cell 1   | Cell 2   |";

        let ast = parser.parse(&arena, source).unwrap();

        let table = &ast.children[0];
        assert_eq!(table.node_type, NodeType::Table);
        assert!(!table.children.is_empty());

        let first_row = &table.children[0];
        assert_eq!(first_row.node_type, NodeType::TableRow);
    }

    #[test]
    fn test_parse_html_inline() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "<div>HTML content</div>";

        let ast = parser.parse(&arena, source).unwrap();

        let has_html = ast.children.iter().any(|c| c.node_type == NodeType::Html);
        assert!(has_html);
    }

    #[test]
    fn test_parse_multiple_headings() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.children.len(), 6);

        for (i, child) in ast.children.iter().enumerate() {
            assert_eq!(child.node_type, NodeType::Header);
            assert_eq!(child.data.depth, Some((i + 1) as u8));
        }
    }

    #[test]
    fn test_parse_nested_emphasis() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "***bold and italic***";

        let ast = parser.parse(&arena, source).unwrap();

        // Should have nested Strong and Emphasis
        let paragraph = &ast.children[0];
        assert!(!paragraph.children.is_empty());
    }

    #[test]
    fn test_parser_name() {
        let parser = MarkdownParser::new();
        assert_eq!(parser.name(), "markdown");
    }

    #[test]
    fn test_parser_default() {
        let parser = MarkdownParser;
        assert_eq!(parser.name(), "markdown");
    }

    #[test]
    fn test_span_positions() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Hello";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.span.start, 0);
        assert_eq!(ast.span.end, 5);
    }

    #[test]
    fn test_parse_whitespace_only() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "   \n  \n  ";

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        // Whitespace-only should result in empty or minimal children
    }

    #[test]
    fn test_parse_mixed_content() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = r#"# Title

This is a paragraph with **bold** and *italic*.

- List item 1
- List item 2

```rust
fn main() {}
```

> A blockquote

---
"#;

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        // Should have multiple top-level children
        assert!(ast.children.len() >= 5);
    }

    #[test]
    fn test_parse_very_long_line() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let long_text = "A".repeat(10000);
        let source = format!("# {}", long_text);

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        assert!(!ast.children.is_empty());
    }

    #[test]
    fn test_parse_unicode_content() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "# 日本語のタイトル\n\n本文です。";

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        assert!(!ast.children.is_empty());
    }

    #[test]
    fn test_parse_nested_lists() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = r#"- Item 1
  - Nested 1
  - Nested 2
- Item 2"#;

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        assert!(!ast.children.is_empty());
    }

    #[test]
    fn test_parse_definition_references() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = r#"[link][ref]

[ref]: https://example.com "Title""#;

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        // Should parse both the reference and definition
        assert!(ast.children.len() >= 1);
    }

    #[test]
    fn test_parse_autolink() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "<https://example.com>";

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
    }

    #[test]
    fn test_parse_task_list() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = r#"- [ ] Unchecked
- [x] Checked"#;

        let ast = parser.parse(&arena, source).unwrap();
        assert_eq!(ast.node_type, NodeType::Document);
        // GFM task lists should be parsed
        assert!(!ast.children.is_empty());
    }

    #[test]
    fn test_extensions_case_insensitive() {
        let parser = MarkdownParser::new();
        assert!(parser.can_parse("MD"));
        assert!(parser.can_parse("Md"));
        assert!(parser.can_parse("mD"));
        assert!(parser.can_parse("MARKDOWN"));
    }
}