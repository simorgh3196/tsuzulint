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
                self.create_parent_node(arena, node, &root.children, source, NodeType::Document)
            }

            Node::Paragraph(para) => {
                self.create_parent_node(arena, node, &para.children, source, NodeType::Paragraph)
            }

            Node::Heading(heading) => {
                let mut node = self.create_parent_node(
                    arena,
                    node,
                    &heading.children,
                    source,
                    NodeType::Header,
                );
                node.data = NodeData::header(heading.depth);
                node
            }

            Node::Text(text) => {
                self.create_text_node(arena, node, &text.value, source, NodeType::Str)
            }

            Node::Emphasis(em) => {
                self.create_parent_node(arena, node, &em.children, source, NodeType::Emphasis)
            }

            Node::Strong(strong) => {
                self.create_parent_node(arena, node, &strong.children, source, NodeType::Strong)
            }

            Node::InlineCode(code) => {
                self.create_text_node(arena, node, &code.value, source, NodeType::Code)
            }

            Node::Code(code) => {
                let mut node =
                    self.create_text_node(arena, node, &code.value, source, NodeType::CodeBlock);
                if let Some(lang) = &code.lang {
                    node.data = NodeData::code_block(Some(arena.alloc_str(lang)));
                }
                node
            }

            Node::Link(link) => {
                let mut node =
                    self.create_parent_node(arena, node, &link.children, source, NodeType::Link);
                let url = arena.alloc_str(&link.url);
                let title = link.title.as_ref().map(|t| arena.alloc_str(t));
                node.data = NodeData::link(url, title);
                node
            }

            Node::Image(image) => {
                let mut node = self.create_leaf_node(node, source, NodeType::Image);
                let url = arena.alloc_str(&image.url);
                let title = image.title.as_ref().map(|t| arena.alloc_str(t));
                node.data = NodeData::link(url, title);
                node
            }

            Node::List(list) => {
                let mut node =
                    self.create_parent_node(arena, node, &list.children, source, NodeType::List);
                node.data = NodeData::list(list.ordered);
                node
            }

            Node::ListItem(item) => {
                self.create_parent_node(arena, node, &item.children, source, NodeType::ListItem)
            }

            Node::Blockquote(quote) => {
                self.create_parent_node(arena, node, &quote.children, source, NodeType::BlockQuote)
            }

            Node::ThematicBreak(_) => self.create_leaf_node(node, source, NodeType::HorizontalRule),

            Node::Break(_) => self.create_leaf_node(node, source, NodeType::Break),

            Node::Html(html) => {
                self.create_text_node(arena, node, &html.value, source, NodeType::Html)
            }

            Node::Delete(del) => {
                self.create_parent_node(arena, node, &del.children, source, NodeType::Delete)
            }

            // Table support (GFM)
            Node::Table(table) => {
                self.create_parent_node(arena, node, &table.children, source, NodeType::Table)
            }

            Node::TableRow(row) => {
                self.create_parent_node(arena, node, &row.children, source, NodeType::TableRow)
            }

            Node::TableCell(cell) => {
                self.create_parent_node(arena, node, &cell.children, source, NodeType::TableCell)
            }

            // Footnotes (GFM)
            Node::FootnoteDefinition(def) => {
                let mut node = self.create_parent_node(
                    arena,
                    node,
                    &def.children,
                    source,
                    NodeType::FootnoteDefinition,
                );
                node.data.identifier = Some(arena.alloc_str(&def.identifier));
                if let Some(label) = &def.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            Node::FootnoteReference(ref_node) => {
                let mut node = self.create_leaf_node(node, source, NodeType::FootnoteReference);
                node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
                if let Some(label) = &ref_node.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            // Reference nodes
            Node::LinkReference(ref_node) => {
                let mut node = self.create_parent_node(
                    arena,
                    node,
                    &ref_node.children,
                    source,
                    NodeType::LinkReference,
                );
                node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
                if let Some(label) = &ref_node.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            Node::ImageReference(ref_node) => {
                let mut node = self.create_leaf_node(node, source, NodeType::ImageReference);
                node.data.identifier = Some(arena.alloc_str(&ref_node.identifier));
                if let Some(label) = &ref_node.label {
                    node.data.label = Some(arena.alloc_str(label));
                }
                node
            }

            Node::Definition(def) => {
                let mut node = self.create_leaf_node(node, source, NodeType::Definition);
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
            _ => self.create_leaf_node(node, source, NodeType::Html),
        }
    }

    /// Helper to create a parent node.
    fn create_parent_node<'a>(
        &self,
        arena: &'a AstArena,
        node: &markdown::mdast::Node,
        children: &[markdown::mdast::Node],
        source: &str,
        node_type: NodeType,
    ) -> TxtNode<'a> {
        let children = self.convert_children(arena, children, source);
        let span = self.node_span(node, source);
        TxtNode::new_parent(node_type, span, children)
    }

    /// Helper to create a text node.
    fn create_text_node<'a>(
        &self,
        arena: &'a AstArena,
        node: &markdown::mdast::Node,
        text: &str,
        source: &str,
        node_type: NodeType,
    ) -> TxtNode<'a> {
        let span = self.node_span(node, source);
        let value = arena.alloc_str(text);
        TxtNode::new_text(node_type, span, value)
    }

    /// Helper to create a leaf node.
    fn create_leaf_node<'a>(
        &self,
        node: &markdown::mdast::Node,
        source: &str,
        node_type: NodeType,
    ) -> TxtNode<'a> {
        let span = self.node_span(node, source);
        TxtNode::new_leaf(node_type, span)
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
    fn test_parse_hard_break() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Line one  \nLine two";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_break = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Break);

        assert!(has_break);
    }

    #[test]
    fn test_parse_footnote_definition() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Text with footnote[^1]\n\n[^1]: This is the footnote content";

        let ast = parser.parse(&arena, source).unwrap();

        let has_footnote_def = ast
            .children
            .iter()
            .any(|c| c.node_type == NodeType::FootnoteDefinition);

        assert!(has_footnote_def);

        if let Some(footnote_def) = ast
            .children
            .iter()
            .find(|c| c.node_type == NodeType::FootnoteDefinition)
        {
            assert_eq!(footnote_def.data.identifier, Some("1"));
        }
    }

    #[test]
    fn test_parse_footnote_reference() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Text with footnote[^note]";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_footnote_ref = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::FootnoteReference);

        assert!(has_footnote_ref);

        if let Some(footnote_ref) = paragraph
            .children
            .iter()
            .find(|c| c.node_type == NodeType::FootnoteReference)
        {
            assert_eq!(footnote_ref.data.identifier, Some("note"));
        }
    }

    #[test]
    fn test_parse_link_reference() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "[Link text][ref]\n\n[ref]: https://example.com";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_link_ref = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::LinkReference);

        assert!(has_link_ref);

        if let Some(link_ref) = paragraph
            .children
            .iter()
            .find(|c| c.node_type == NodeType::LinkReference)
        {
            assert_eq!(link_ref.data.identifier, Some("ref"));
        }
    }

    #[test]
    fn test_parse_image_reference() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "![Alt text][img-ref]\n\n[img-ref]: image.png";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_image_ref = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::ImageReference);

        assert!(has_image_ref);

        if let Some(image_ref) = paragraph
            .children
            .iter()
            .find(|c| c.node_type == NodeType::ImageReference)
        {
            assert_eq!(image_ref.data.identifier, Some("img-ref"));
        }
    }

    #[test]
    fn test_parse_definition() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "[ref]: https://example.com \"Example Title\"";

        let ast = parser.parse(&arena, source).unwrap();

        let has_definition = ast
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Definition);

        assert!(has_definition);

        if let Some(definition) = ast
            .children
            .iter()
            .find(|c| c.node_type == NodeType::Definition)
        {
            assert_eq!(definition.data.identifier, Some("ref"));
            assert_eq!(definition.data.url, Some("https://example.com"));
            assert_eq!(definition.data.title, Some("Example Title"));
        }
    }

    #[test]
    fn test_parse_definition_without_title() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "[ref]: https://example.com";

        let ast = parser.parse(&arena, source).unwrap();

        if let Some(definition) = ast
            .children
            .iter()
            .find(|c| c.node_type == NodeType::Definition)
        {
            assert_eq!(definition.data.identifier, Some("ref"));
            assert_eq!(definition.data.url, Some("https://example.com"));
            assert!(definition.data.title.is_none());
        }
    }

    #[test]
    fn test_parse_nested_list_in_blockquote() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "> - Item 1\n> - Item 2";

        let ast = parser.parse(&arena, source).unwrap();

        let blockquote = &ast.children[0];
        assert_eq!(blockquote.node_type, NodeType::BlockQuote);

        let list = &blockquote.children[0];
        assert_eq!(list.node_type, NodeType::List);
        assert_eq!(list.children.len(), 2);
    }

    #[test]
    fn test_parse_nested_blockquotes() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "> Level 1\n>> Level 2";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.children[0].node_type, NodeType::BlockQuote);
    }

    #[test]
    fn test_parse_list_with_paragraphs() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "- Item with paragraph\n\n  Second paragraph in item";

        let ast = parser.parse(&arena, source).unwrap();

        let list = &ast.children[0];
        assert_eq!(list.node_type, NodeType::List);

        let list_item = &list.children[0];
        assert_eq!(list_item.node_type, NodeType::ListItem);
        assert!(list_item.children.len() > 1);
    }

    #[test]
    fn test_parse_complex_document() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = r#"# Document Title

This is a paragraph with **bold** and *italic* text.

## Section 1

- List item 1
- List item 2
  - Nested item

```rust
fn main() {
    println!("Hello");
}
```

[Link](https://example.com)

> Quote with **emphasis**

---

### Subsection

1. Ordered item 1
2. Ordered item 2
"#;

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.node_type, NodeType::Document);
        assert!(ast.children.len() > 5);

        // Verify various node types exist
        let has_header = ast.children.iter().any(|c| c.node_type == NodeType::Header);
        let has_list = ast.children.iter().any(|c| c.node_type == NodeType::List);
        let has_code_block = ast
            .children
            .iter()
            .any(|c| c.node_type == NodeType::CodeBlock);
        let has_blockquote = ast
            .children
            .iter()
            .any(|c| c.node_type == NodeType::BlockQuote);
        let has_hr = ast
            .children
            .iter()
            .any(|c| c.node_type == NodeType::HorizontalRule);

        assert!(has_header);
        assert!(has_list);
        assert!(has_code_block);
        assert!(has_blockquote);
        assert!(has_hr);
    }

    #[test]
    fn test_parse_image_without_title() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "![Alt text](image.png)";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let image = &paragraph.children[0];

        assert_eq!(image.node_type, NodeType::Image);
        assert_eq!(image.data.url, Some("image.png"));
        assert!(image.data.title.is_none());
    }

    #[test]
    fn test_parse_multiple_paragraphs() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.children.len(), 3);
        for child in ast.children.iter() {
            assert_eq!(child.node_type, NodeType::Paragraph);
        }
    }

    #[test]
    fn test_parse_mixed_emphasis() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Text with *emphasis* and **strong** and `code` together";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_emphasis = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Emphasis);
        let has_strong = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Strong);
        let has_code = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Code);

        assert!(has_emphasis);
        assert!(has_strong);
        assert!(has_code);
    }

    #[test]
    fn test_parse_table_with_cells() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";

        let ast = parser.parse(&arena, source).unwrap();

        let table = &ast.children[0];
        assert_eq!(table.node_type, NodeType::Table);
        assert!(table.children.len() >= 2);

        for row in table.children.iter() {
            assert_eq!(row.node_type, NodeType::TableRow);
            for cell in row.children.iter() {
                assert_eq!(cell.node_type, NodeType::TableCell);
            }
        }
    }

    #[test]
    fn test_parse_link_with_inline_emphasis() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "[**Bold link**](https://example.com)";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let link = &paragraph.children[0];

        assert_eq!(link.node_type, NodeType::Link);
        assert_eq!(link.data.url, Some("https://example.com"));
        assert!(!link.children.is_empty());

        let has_strong = link.children.iter().any(|c| c.node_type == NodeType::Strong);
        assert!(has_strong);
    }

    #[test]
    fn test_parse_code_block_with_newlines() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "```python\ndef hello():\n    print(\"Hello, World!\")\n```";

        let ast = parser.parse(&arena, source).unwrap();

        let code_block = &ast.children[0];
        assert_eq!(code_block.node_type, NodeType::CodeBlock);
        assert_eq!(code_block.data.lang, Some("python"));
        assert!(code_block.value.is_some());

        if let Some(value) = code_block.value {
            assert!(value.contains("def hello()"));
            assert!(value.contains("print"));
        }
    }

    #[test]
    fn test_parse_autolink() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "<https://example.com>";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        let has_link = paragraph
            .children
            .iter()
            .any(|c| c.node_type == NodeType::Link);

        assert!(has_link);
    }

    #[test]
    fn test_parse_whitespace_only() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "   \n\n   \n";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.node_type, NodeType::Document);
        // Should handle whitespace gracefully
    }

    #[test]
    fn test_parse_heading_with_inline_code() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "# Heading with `code`";

        let ast = parser.parse(&arena, source).unwrap();

        let heading = &ast.children[0];
        assert_eq!(heading.node_type, NodeType::Header);
        assert_eq!(heading.data.depth, Some(1));

        let has_code = heading.children.iter().any(|c| c.node_type == NodeType::Code);
        assert!(has_code);
    }

    #[test]
    fn test_parse_deeply_nested_list() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "- Level 1\n  - Level 2\n    - Level 3";

        let ast = parser.parse(&arena, source).unwrap();

        let list = &ast.children[0];
        assert_eq!(list.node_type, NodeType::List);
        assert!(!list.children.is_empty());
    }

    #[test]
    fn test_parse_mixed_list_types() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "1. Ordered\n   - Unordered nested\n2. Ordered again";

        let ast = parser.parse(&arena, source).unwrap();

        let list = &ast.children[0];
        assert_eq!(list.node_type, NodeType::List);
        assert_eq!(list.data.ordered, Some(true));
    }

    #[test]
    fn test_parse_blockquote_with_code() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "> ```\n> code in quote\n> ```";

        let ast = parser.parse(&arena, source).unwrap();

        let blockquote = &ast.children[0];
        assert_eq!(blockquote.node_type, NodeType::BlockQuote);
    }

    #[test]
    fn test_parser_extensions() {
        let parser = MarkdownParser::new();
        let extensions = parser.extensions();

        assert_eq!(extensions.len(), 5);
        assert!(extensions.contains(&"md"));
        assert!(extensions.contains(&"markdown"));
        assert!(extensions.contains(&"mdown"));
        assert!(extensions.contains(&"mkdn"));
        assert!(extensions.contains(&"mkd"));
    }

    #[test]
    fn test_parse_all_heading_levels() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();

        for level in 1..=6 {
            let source = format!("{} Heading", "#".repeat(level));
            let ast = parser.parse(&arena, &source).unwrap();

            assert_eq!(ast.children.len(), 1);
            let heading = &ast.children[0];
            assert_eq!(heading.node_type, NodeType::Header);
            assert_eq!(heading.data.depth, Some(level as u8));
        }
    }

    #[test]
    fn test_parse_unicode_content() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "# 日本語のタイトル\n\n本文です。**太字**と*斜体*。";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.node_type, NodeType::Document);
        assert!(!ast.children.is_empty());

        let heading = &ast.children[0];
        assert_eq!(heading.node_type, NodeType::Header);
    }

    #[test]
    fn test_parse_emoji() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Text with emoji 🚀 and more text";

        let ast = parser.parse(&arena, source).unwrap();

        let paragraph = &ast.children[0];
        assert_eq!(paragraph.node_type, NodeType::Paragraph);
        assert!(!paragraph.children.is_empty());
    }

    #[test]
    fn test_parse_special_characters() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Text with & < > \" ' special chars";

        let ast = parser.parse(&arena, source).unwrap();

        assert_eq!(ast.node_type, NodeType::Document);
        assert!(!ast.children.is_empty());
    }

    #[test]
    fn test_parse_error_invalid_markdown() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        // markdown-rs is very lenient, so we test that parse returns Ok even for edge cases
        let source = "[unclosed link";

        let result = parser.parse(&arena, source);
        // Should still parse successfully (markdown is forgiving)
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_node_helpers() {
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let source = "Test text";

        let ast = parser.parse(&arena, source).unwrap();

        // Verify the helper methods created valid nodes
        assert_eq!(ast.node_type, NodeType::Document);
        assert!(ast.span.start <= ast.span.end);

        if !ast.children.is_empty() {
            let paragraph = &ast.children[0];
            assert_eq!(paragraph.node_type, NodeType::Paragraph);
            assert!(paragraph.span.start <= paragraph.span.end);

            if !paragraph.children.is_empty() {
                let text_node = &paragraph.children[0];
                assert_eq!(text_node.node_type, NodeType::Str);
                assert!(text_node.value.is_some());
            }
        }
    }
}