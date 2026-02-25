//! Lint context for caching parse results.
//!
//! This module provides efficient access to parsed document information,
//! avoiding redundant computations during linting.

use std::cell::OnceCell;

use tsuzulint_ast::{NodeType, Span, TxtNode};

/// Pre-computed metadata for a single line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineInfo {
    /// Byte offset of line start (inclusive).
    pub start: u32,
    /// Byte offset of end of line content, excluding the newline character.
    pub end: u32,
    /// Indentation level in spaces (tabs count as 4 spaces).
    pub indent: u32,
    /// Byte length of leading whitespace.
    pub indent_bytes: u32,
    /// Whether this line contains only whitespace.
    pub is_blank: bool,
}

impl LineInfo {
    /// Creates a new LineInfo from a line's content.
    pub fn from_line(start: u32, line_text: &str) -> Self {
        let end = start + line_text.len() as u32;
        let trimmed = line_text.trim_end();
        let is_blank = trimmed.is_empty();
        let (indent, indent_bytes) = if is_blank {
            (0, 0)
        } else {
            let leading_len = line_text.len() - line_text.trim_start().len();
            let leading_text = &line_text[..leading_len];
            let visual = leading_text.chars().fold(0u32, |acc, c| {
                if c == '\t' {
                    (acc + 4) / 4 * 4
                } else {
                    acc + 1
                }
            });
            (visual, leading_len as u32)
        };

        Self {
            start,
            end,
            indent,
            indent_bytes,
            is_blank,
        }
    }

    /// Returns the byte offset of the first non-whitespace character.
    pub fn content_start(&self) -> u32 {
        self.start + self.indent_bytes
    }
}

/// Cached document structure for efficient querying.
#[derive(Debug, Default)]
pub struct DocumentStructure {
    /// All headings in the document.
    pub headings: Vec<HeadingInfo>,
    /// All links in the document.
    pub links: Vec<LinkInfo>,
    /// All code blocks in the document.
    pub code_blocks: Vec<CodeBlockInfo>,
}

/// Information about a heading node.
#[derive(Debug, Clone)]
pub struct HeadingInfo {
    /// Heading level (1-6).
    pub depth: u8,
    /// Span of the heading.
    pub span: Span,
    /// Heading text content.
    pub text: String,
}

/// Information about a link node.
#[derive(Debug, Clone)]
pub struct LinkInfo {
    /// Link URL.
    pub url: String,
    /// Link title (if present).
    pub title: Option<String>,
    /// Span of the link.
    pub span: Span,
    /// Whether this is an image link.
    pub is_image: bool,
}

/// Information about a code block node.
#[derive(Debug, Clone)]
pub struct CodeBlockInfo {
    /// Language identifier (if present).
    pub lang: Option<String>,
    /// Span of the code block.
    pub span: Span,
    /// Whether this is an inline code span.
    pub is_inline: bool,
}

/// Pre-analyzed content characteristics for early rule filtering.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContentCharacteristics {
    pub has_headings: bool,
    pub has_links: bool,
    pub has_images: bool,
    pub has_code_blocks: bool,
    pub has_fenced_code: bool,
    pub has_inline_code: bool,
    pub has_lists: bool,
    pub has_tables: bool,
    pub has_blockquotes: bool,
    pub has_html: bool,
}

impl ContentCharacteristics {
    /// Analyze content in a single pass.
    pub fn analyze(source: &str) -> Self {
        let mut chars = Self::default();
        let mut prev_non_blank = false;

        for line in source.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('#') {
                chars.has_headings = true;
            }
            if !chars.has_headings
                && prev_non_blank
                && !trimmed.is_empty()
                && (trimmed.bytes().all(|b| b == b'=') || trimmed.bytes().all(|b| b == b'-'))
            {
                chars.has_headings = true;
            }
            if trimmed.contains('[') && trimmed.contains(']') {
                chars.has_links = true;
            }
            if trimmed.contains("![") {
                chars.has_images = true;
            }
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                chars.has_fenced_code = true;
            } else if trimmed.contains('`') {
                chars.has_inline_code = true;
            }
            if line.starts_with("    ") || line.starts_with('\t') {
                chars.has_code_blocks = true;
            }
            if trimmed.starts_with('-') || trimmed.starts_with('*') || trimmed.starts_with('+') {
                chars.has_lists = true;
            } else if !chars.has_lists {
                let mut has_digit = false;
                for c in trimmed.chars() {
                    if c.is_ascii_digit() {
                        has_digit = true;
                    } else if has_digit && (c == '.' || c == ')') {
                        chars.has_lists = true;
                        break;
                    } else {
                        break;
                    }
                }
            }
            if trimmed.contains('|') && trimmed.contains("---") {
                chars.has_tables = true;
            }
            if trimmed.starts_with('>') {
                chars.has_blockquotes = true;
            }
            if trimmed.starts_with('<') {
                chars.has_html = true;
            }
            prev_non_blank = !trimmed.is_empty();
        }

        chars
    }

    /// Check if rule should be skipped based on characteristics.
    ///
    /// Returns true only if ALL specified node types are absent from the document.
    /// This allows rules that handle multiple node types to run if ANY type exists.
    pub fn should_skip_rule(&self, node_types: &[String]) -> bool {
        if node_types.is_empty() {
            return false;
        }

        node_types.iter().all(|node_type| match node_type.as_str() {
            "Header" | "Heading" | "heading" => !self.has_headings,
            "Link" | "link" => !self.has_links,
            "Image" | "image" => !self.has_images,
            "CodeBlock" => !self.has_code_blocks && !self.has_fenced_code,
            "Code" | "code" => !self.has_inline_code,
            "List" | "list" => !self.has_lists,
            "Table" | "table" => !self.has_tables,
            "BlockQuote" | "Blockquote" | "blockquote" => !self.has_blockquotes,
            "Html" | "html" => !self.has_html,
            _ => false,
        })
    }
}

/// Context for linting a document.
///
/// Provides cached access to line information and document structure,
/// avoiding redundant computations.
pub struct LintContext<'a> {
    /// The source text.
    source: &'a str,
    /// Pre-computed line information.
    lines: Vec<LineInfo>,
    /// Lazily constructed document structure.
    structure: OnceCell<DocumentStructure>,
    /// Pre-analyzed content characteristics for early filtering.
    characteristics: ContentCharacteristics,
}

impl<'a> LintContext<'a> {
    /// Creates a new LintContext from source text.
    pub fn new(source: &'a str) -> Self {
        let lines = Self::compute_lines(source);
        let characteristics = ContentCharacteristics::analyze(source);
        Self {
            source,
            lines,
            structure: OnceCell::new(),
            characteristics,
        }
    }

    /// Creates a new LintContext with a pre-parsed AST.
    ///
    /// This automatically calls [`build_structure()`](Self::build_structure) to populate
    /// the document structure cache from the AST.
    pub fn with_ast(source: &'a str, root: &TxtNode<'a>) -> Self {
        let ctx = Self::new(source);
        ctx.build_structure(root);
        ctx
    }

    /// Returns the cached document structure.
    ///
    /// Returns an empty structure if neither [`with_ast()`](Self::with_ast) nor
    /// [`build_structure()`](Self::build_structure) was called before this method.
    /// See [`build_structure()`](Self::build_structure) for ordering requirements.
    pub fn structure(&self) -> &DocumentStructure {
        self.structure.get_or_init(DocumentStructure::default)
    }

    /// Computes line information from source text.
    fn compute_lines(source: &str) -> Vec<LineInfo> {
        let mut lines = Vec::new();
        let mut offset = 0u32;

        for line in source.lines() {
            let info = LineInfo::from_line(offset, line);
            lines.push(info);
            offset = info.end;
            if (offset as usize) < source.len() {
                let remaining = &source.as_bytes()[offset as usize..];
                if remaining.starts_with(b"\r\n") {
                    offset += 2;
                } else {
                    offset += 1;
                }
            }
        }

        if source.ends_with('\n')
            && let Some(last) = lines.last()
        {
            let newline_len = if source.ends_with("\r\n") { 2 } else { 1 };
            if last.end == source.len() as u32 - newline_len {
                lines.push(LineInfo {
                    start: source.len() as u32,
                    end: source.len() as u32,
                    indent: 0,
                    indent_bytes: 0,
                    is_blank: true,
                });
            }
        }

        lines
    }

    /// Returns the source text.
    pub fn source(&self) -> &'a str {
        self.source
    }

    /// Returns the number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Returns line information for the given line number (1-indexed).
    pub fn line_info(&self, line: u32) -> Option<&LineInfo> {
        if line == 0 {
            return None;
        }
        self.lines.get(line as usize - 1)
    }

    /// Converts a byte offset to a 1-indexed line number.
    ///
    /// Uses binary search for O(log n) lookup.
    pub fn byte_offset_to_line(&self, offset: u32) -> Option<u32> {
        if self.lines.is_empty() {
            return None;
        }

        let idx = self.lines.partition_point(|info| info.start <= offset);

        if idx == 0 {
            return None;
        }

        let line_idx = idx - 1;
        let info = &self.lines[line_idx];

        let in_line = offset <= info.end
            || (line_idx + 1 < self.lines.len() && offset < self.lines[line_idx + 1].start)
            || (line_idx + 1 == self.lines.len() && offset <= self.source.len() as u32);

        if in_line {
            Some((line_idx + 1) as u32)
        } else {
            None
        }
    }

    /// Returns the text of a specific line (1-indexed).
    pub fn line_text(&self, line: u32) -> Option<&'a str> {
        let info = self.line_info(line)?;
        let end = if info.end > info.start && (info.end as usize) <= self.source.len() {
            if self.source.as_bytes()[(info.end as usize) - 1] == b'\r' {
                info.end - 1
            } else {
                info.end
            }
        } else {
            info.end
        };
        Some(&self.source[info.start as usize..end as usize])
    }

    /// Extracts text content from a node and its children using iterative traversal.
    fn extract_text(node: &TxtNode<'a>) -> String {
        let mut text = String::new();
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if let Some(v) = n.value {
                text.push_str(v);
            }
            stack.extend(n.children.iter().rev());
        }
        text
    }

    /// Builds document structure from an AST node.
    ///
    /// # Ordering Requirement
    ///
    /// This method must be called **before** [`structure()`](Self::structure) to populate
    /// the cached structure with AST data. If `structure()` is called first, it will
    /// initialize the cache with an empty `DocumentStructure::default()`, and subsequent
    /// calls to `build_structure()` will have no effect.
    ///
    /// For convenience, use [`with_ast()`](Self::with_ast) to construct a context that
    /// automatically builds structure from the AST.
    pub fn build_structure(&self, root: &TxtNode<'a>) -> &DocumentStructure {
        self.structure.get_or_init(|| {
            let mut structure = DocumentStructure::default();
            Self::collect_structure(root, &mut structure);
            structure
        })
    }

    fn collect_structure(node: &TxtNode<'a>, structure: &mut DocumentStructure) {
        match node.node_type {
            NodeType::Header => {
                let depth = node.depth().unwrap_or(1);
                let text = Self::extract_text(node);
                structure.headings.push(HeadingInfo {
                    depth,
                    span: node.span,
                    text,
                });
            }
            NodeType::Link | NodeType::Image => {
                let url = node.url().unwrap_or("").to_string();
                let title = node.title().map(|s| s.to_string());
                structure.links.push(LinkInfo {
                    url,
                    title,
                    span: node.span,
                    is_image: node.node_type == NodeType::Image,
                });
            }
            NodeType::CodeBlock => {
                let lang = node.lang().map(|s| s.to_string());
                structure.code_blocks.push(CodeBlockInfo {
                    lang,
                    span: node.span,
                    is_inline: false,
                });
            }
            NodeType::Code => {
                structure.code_blocks.push(CodeBlockInfo {
                    lang: None,
                    span: node.span,
                    is_inline: true,
                });
            }
            _ => {}
        }

        for child in node.children {
            Self::collect_structure(child, structure);
        }
    }

    /// Checks if a byte offset is within a code block.
    pub fn is_in_code_block(&self, offset: u32, structure: &DocumentStructure) -> bool {
        structure
            .code_blocks
            .iter()
            .any(|cb| !cb.is_inline && cb.span.start <= offset && offset < cb.span.end)
    }

    /// Returns the pre-analyzed content characteristics.
    pub fn characteristics(&self) -> &ContentCharacteristics {
        &self.characteristics
    }

    /// Check if rule should be skipped (early filtering).
    pub fn should_skip_rule(&self, node_types: &[String]) -> bool {
        self.characteristics.should_skip_rule(node_types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_info_simple() {
        let info = LineInfo::from_line(0, "hello");
        assert_eq!(info.start, 0);
        assert_eq!(info.end, 5);
        assert_eq!(info.indent, 0);
        assert!(!info.is_blank);
    }

    #[test]
    fn test_line_info_indented() {
        let info = LineInfo::from_line(10, "    hello");
        assert_eq!(info.start, 10);
        assert_eq!(info.end, 19);
        assert_eq!(info.indent, 4);
        assert!(!info.is_blank);
    }

    #[test]
    fn test_line_info_blank() {
        let info = LineInfo::from_line(0, "");
        assert_eq!(info.start, 0);
        assert_eq!(info.end, 0);
        assert!(info.is_blank);
        assert_eq!(info.indent, 0);
    }

    #[test]
    fn test_line_info_whitespace_only() {
        let info = LineInfo::from_line(0, "   ");
        assert_eq!(info.start, 0);
        assert_eq!(info.end, 3);
        assert!(info.is_blank);
    }

    #[test]
    fn test_line_info_tab_indent() {
        let info = LineInfo::from_line(0, "\thello");
        assert_eq!(info.indent, 4);
    }

    #[test]
    fn test_lint_context_empty() {
        let ctx = LintContext::new("");
        assert_eq!(ctx.line_count(), 0);
    }

    #[test]
    fn test_lint_context_single_line() {
        let ctx = LintContext::new("hello");
        assert_eq!(ctx.line_count(), 1);
        assert_eq!(ctx.line_info(1).unwrap().start, 0);
        assert_eq!(ctx.line_info(1).unwrap().end, 5);
    }

    #[test]
    fn test_lint_context_multiple_lines() {
        let ctx = LintContext::new("hello\nworld\nfoo");
        assert_eq!(ctx.line_count(), 3);

        assert_eq!(ctx.line_info(1).unwrap().start, 0);
        assert_eq!(ctx.line_info(1).unwrap().end, 5);

        assert_eq!(ctx.line_info(2).unwrap().start, 6);
        assert_eq!(ctx.line_info(2).unwrap().end, 11);

        assert_eq!(ctx.line_info(3).unwrap().start, 12);
        assert_eq!(ctx.line_info(3).unwrap().end, 15);
    }

    #[test]
    fn test_byte_offset_to_line_simple() {
        let ctx = LintContext::new("hello\nworld");

        assert_eq!(ctx.byte_offset_to_line(0), Some(1));
        assert_eq!(ctx.byte_offset_to_line(4), Some(1));
        assert_eq!(ctx.byte_offset_to_line(5), Some(1));
        assert_eq!(ctx.byte_offset_to_line(6), Some(2));
        assert_eq!(ctx.byte_offset_to_line(10), Some(2));
        assert_eq!(ctx.byte_offset_to_line(11), Some(2));
    }

    #[test]
    fn test_byte_offset_to_line_empty() {
        let ctx = LintContext::new("");
        assert_eq!(ctx.byte_offset_to_line(0), None);
    }

    #[test]
    fn test_byte_offset_to_line_out_of_bounds() {
        let ctx = LintContext::new("hello");
        assert_eq!(ctx.byte_offset_to_line(100), None);
    }

    #[test]
    fn test_line_text() {
        let ctx = LintContext::new("hello\nworld\nfoo");

        assert_eq!(ctx.line_text(1), Some("hello"));
        assert_eq!(ctx.line_text(2), Some("world"));
        assert_eq!(ctx.line_text(3), Some("foo"));
        assert_eq!(ctx.line_text(4), None);
    }

    #[test]
    fn test_line_text_with_trailing_newline() {
        let ctx = LintContext::new("hello\nworld\n");
        assert_eq!(ctx.line_text(1), Some("hello"));
        assert_eq!(ctx.line_text(2), Some("world"));
    }

    #[test]
    fn test_content_start() {
        let info = LineInfo::from_line(10, "    hello");
        assert_eq!(info.content_start(), 14);
    }

    #[test]
    fn test_content_start_tab_indent() {
        // Tab is 1 byte, so content_start() must return start + 1, not start + 4.
        let info = LineInfo::from_line(10, "\thello");
        assert_eq!(info.indent, 4); // visual indent
        assert_eq!(info.indent_bytes, 1); // byte length of leading whitespace
        assert_eq!(info.content_start(), 11); // 10 + 1 byte, not 10 + 4
    }

    #[test]
    fn test_line_info_with_cr() {
        let info = LineInfo::from_line(0, "hello\r");
        assert_eq!(info.end, 6);
        assert!(!info.is_blank);
    }

    #[test]
    fn test_multiple_tabs() {
        let info = LineInfo::from_line(0, "\t\thello");
        assert_eq!(info.indent, 8);
    }

    #[test]
    fn test_mixed_indent() {
        let info = LineInfo::from_line(0, "  \thello");
        assert_eq!(info.indent, 4);
    }

    #[test]
    fn test_lint_context_trailing_newline() {
        let ctx = LintContext::new("hello\n");
        assert_eq!(ctx.line_count(), 2);
        assert_eq!(ctx.line_info(1).unwrap().end, 5);
    }

    #[test]
    fn test_lint_context_crlf() {
        let ctx = LintContext::new("hello\r\nworld\r\n");
        assert_eq!(ctx.line_count(), 3);
        assert_eq!(ctx.line_text(1), Some("hello"));
        assert_eq!(ctx.line_text(2), Some("world"));
        // Byte offset after "hello\r\n" = 7
        assert_eq!(ctx.line_info(2).unwrap().start, 7);
        assert_eq!(ctx.line_info(2).unwrap().end, 12);
        assert_eq!(ctx.line_info(3).unwrap().start, 14);
        assert_eq!(ctx.line_info(3).unwrap().end, 14);
    }

    #[test]
    fn test_line_info_zero() {
        let ctx = LintContext::new("hello");
        assert_eq!(ctx.line_info(0), None);
    }

    #[test]
    fn test_line_info_multibyte() {
        let ctx = LintContext::new("  こんにちは\n世界");
        assert_eq!(ctx.line_count(), 2);
        assert_eq!(ctx.line_info(1).unwrap().start, 0);
        assert_eq!(ctx.line_info(1).unwrap().end, 17); // 2 spaces + 15 bytes (5 Japanese chars × 3 bytes)
        assert_eq!(ctx.line_info(1).unwrap().indent, 2); // 2 spaces = 2 bytes
    }

    // ---- Tests for with_ast and structure() ----

    use tsuzulint_ast::{AstArena, NodeData};

    fn make_header_node<'a>(
        arena: &'a AstArena,
        depth: u8,
        text: &'a str,
        span: Span,
    ) -> &'a TxtNode<'a> {
        let str_node = arena.alloc(TxtNode::new_text(NodeType::Str, span, text));
        let children = arena.alloc_slice_copy(&[*str_node]);
        let mut header = TxtNode::new_parent(NodeType::Header, span, children);
        header.data = NodeData::header(depth);
        arena.alloc(header)
    }

    fn make_link_node<'a>(
        arena: &'a AstArena,
        url: &'a str,
        title: Option<&'a str>,
        text: &'a str,
        span: Span,
    ) -> &'a TxtNode<'a> {
        let str_node = arena.alloc(TxtNode::new_text(NodeType::Str, span, text));
        let children = arena.alloc_slice_copy(&[*str_node]);
        let mut link = TxtNode::new_parent(NodeType::Link, span, children);
        link.data = NodeData::link(url, title);
        arena.alloc(link)
    }

    fn make_code_block_node<'a>(
        arena: &'a AstArena,
        lang: Option<&'a str>,
        code: &'a str,
        span: Span,
    ) -> &'a TxtNode<'a> {
        let mut cb = TxtNode::new_text(NodeType::CodeBlock, span, code);
        cb.data = NodeData::code_block(lang);
        arena.alloc(cb)
    }

    fn make_inline_code_node<'a>(
        arena: &'a AstArena,
        code: &'a str,
        span: Span,
    ) -> &'a TxtNode<'a> {
        arena.alloc(TxtNode::new_text(NodeType::Code, span, code))
    }

    #[test]
    fn test_with_ast_builds_structure_from_headings() {
        // Source "# Title\n## Sub" is 14 bytes:
        // - "# Title" (7 bytes, span 0-7)
        // - "\n" (1 byte)
        // - "## Sub" (6 bytes, span 8-14)
        let arena = AstArena::new();
        let h1 = make_header_node(&arena, 1, "Title", Span::new(0, 7));
        let h2 = make_header_node(&arena, 2, "Sub", Span::new(8, 14));
        let children = arena.alloc_slice_copy(&[*h1, *h2]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 14),
            children,
        ));

        let ctx = LintContext::with_ast("# Title\n## Sub", doc);
        let structure = ctx.structure();

        assert_eq!(structure.headings.len(), 2);
        assert_eq!(structure.headings[0].depth, 1);
        assert_eq!(structure.headings[0].text, "Title");
        assert_eq!(structure.headings[1].depth, 2);
        assert_eq!(structure.headings[1].text, "Sub");
    }

    #[test]
    fn test_with_ast_builds_structure_from_links() {
        let arena = AstArena::new();
        let link = make_link_node(
            &arena,
            "https://example.com",
            Some("Example"),
            "click",
            Span::new(0, 30),
        );
        let children = arena.alloc_slice_copy(&[*link]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 30),
            children,
        ));

        let ctx = LintContext::with_ast("placeholder source text.......", doc);
        let structure = ctx.structure();

        assert_eq!(structure.links.len(), 1);
        assert_eq!(structure.links[0].url, "https://example.com");
        assert_eq!(structure.links[0].title, Some("Example".to_string()));
        assert!(!structure.links[0].is_image);
    }

    #[test]
    fn test_with_ast_builds_structure_from_image() {
        let arena = AstArena::new();
        let str_node = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 3), "alt"));
        let children = arena.alloc_slice_copy(&[*str_node]);
        let mut img = TxtNode::new_parent(NodeType::Image, Span::new(0, 20), children);
        img.data = NodeData::image("img.png", None);
        let img_ref = arena.alloc(img);
        let doc_children = arena.alloc_slice_copy(&[*img_ref]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 20),
            doc_children,
        ));

        let ctx = LintContext::with_ast("![alt](img.png)     ", doc);
        let structure = ctx.structure();

        assert_eq!(structure.links.len(), 1);
        assert!(structure.links[0].is_image);
        assert_eq!(structure.links[0].url, "img.png");
    }

    #[test]
    fn test_with_ast_builds_structure_from_code_blocks() {
        let arena = AstArena::new();
        let cb = make_code_block_node(&arena, Some("rust"), "fn main() {}", Span::new(0, 30));
        let inline = make_inline_code_node(&arena, "x", Span::new(35, 38));
        let children = arena.alloc_slice_copy(&[*cb, *inline]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 38),
            children,
        ));

        let source = "```rust\nfn main() {}\n```\n\n`x`  extra";
        let ctx = LintContext::with_ast(source, doc);
        let structure = ctx.structure();

        assert_eq!(structure.code_blocks.len(), 2);
        assert!(!structure.code_blocks[0].is_inline);
        assert_eq!(structure.code_blocks[0].lang, Some("rust".to_string()));
        assert!(structure.code_blocks[1].is_inline);
        assert!(structure.code_blocks[1].lang.is_none());
    }

    #[test]
    fn test_structure_without_ast_returns_empty() {
        let ctx = LintContext::new("hello world");
        let structure = ctx.structure();

        assert!(structure.headings.is_empty());
        assert!(structure.links.is_empty());
        assert!(structure.code_blocks.is_empty());
    }

    #[test]
    fn test_structure_is_cached() {
        let arena = AstArena::new();
        let h1 = make_header_node(&arena, 1, "Title", Span::new(0, 7));
        let children = arena.alloc_slice_copy(&[*h1]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 7),
            children,
        ));

        let ctx = LintContext::with_ast("# Title", doc);
        let s1 = ctx.structure() as *const DocumentStructure;
        let s2 = ctx.structure() as *const DocumentStructure;
        // Same pointer means cached
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_is_in_code_block() {
        let arena = AstArena::new();
        let cb = make_code_block_node(&arena, Some("rust"), "code", Span::new(10, 30));
        let inline = make_inline_code_node(&arena, "x", Span::new(35, 40));
        let children = arena.alloc_slice_copy(&[*cb, *inline]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 40),
            children,
        ));

        let source = "          code here             `x`  ext";
        let ctx = LintContext::with_ast(source, doc);
        let structure = ctx.structure();

        // Inside the code block
        assert!(ctx.is_in_code_block(10, structure));
        assert!(ctx.is_in_code_block(20, structure));
        // At the end boundary (exclusive), not inside
        assert!(!ctx.is_in_code_block(30, structure));
        // Before code block
        assert!(!ctx.is_in_code_block(5, structure));
        // Inline code should not count
        assert!(!ctx.is_in_code_block(37, structure));
    }

    #[test]
    fn test_is_in_code_block_multiple_blocks() {
        let arena = AstArena::new();
        let cb1 = make_code_block_node(&arena, None, "a", Span::new(0, 10));
        let cb2 = make_code_block_node(&arena, None, "b", Span::new(20, 30));
        let children = arena.alloc_slice_copy(&[*cb1, *cb2]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 30),
            children,
        ));

        let source = "aaaaaaaaaa          bbbbbbbbbb";
        let ctx = LintContext::with_ast(source, doc);
        let structure = ctx.structure();

        assert!(ctx.is_in_code_block(5, structure));
        assert!(!ctx.is_in_code_block(15, structure)); // gap between blocks
        assert!(ctx.is_in_code_block(25, structure));
    }

    #[test]
    fn test_nested_structure_collection() {
        // Header inside blockquote: collect_structure should traverse children
        let arena = AstArena::new();
        let h1 = make_header_node(&arena, 1, "Nested", Span::new(2, 10));
        let bq_children = arena.alloc_slice_copy(&[*h1]);
        let bq = arena.alloc(TxtNode::new_parent(
            NodeType::BlockQuote,
            Span::new(0, 12),
            bq_children,
        ));
        let doc_children = arena.alloc_slice_copy(&[*bq]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 12),
            doc_children,
        ));

        let ctx = LintContext::with_ast("> # Nested  ", doc);
        let structure = ctx.structure();

        assert_eq!(structure.headings.len(), 1);
        assert_eq!(structure.headings[0].text, "Nested");
    }

    // ---- Additional edge case tests for line operations ----

    #[test]
    fn test_cr_only_line_endings() {
        // Rust's str::lines() only splits on \n and \r\n, not standalone \r.
        // So CR-only content is treated as a single line.
        let ctx = LintContext::new("hello\rworld\rfoo");
        assert_eq!(ctx.line_count(), 1);
    }

    #[test]
    fn test_multiple_consecutive_blank_lines() {
        let ctx = LintContext::new("hello\n\n\nworld");
        assert_eq!(ctx.line_count(), 4);
        assert_eq!(ctx.line_text(1), Some("hello"));
        assert_eq!(ctx.line_text(2), Some(""));
        assert_eq!(ctx.line_text(3), Some(""));
        assert_eq!(ctx.line_text(4), Some("world"));
        assert!(ctx.line_info(2).unwrap().is_blank);
        assert!(ctx.line_info(3).unwrap().is_blank);
    }

    #[test]
    fn test_trailing_crlf() {
        let ctx = LintContext::new("hello\r\n");
        assert_eq!(ctx.line_count(), 2);
        assert_eq!(ctx.line_text(1), Some("hello"));
        // Line 2 is the trailing blank line
        assert!(ctx.line_info(2).unwrap().is_blank);
    }

    #[test]
    fn test_trailing_cr_only() {
        // Rust's str::lines() only splits on \n and \r\n, not standalone \r.
        let ctx = LintContext::new("hello\r");
        assert_eq!(ctx.line_count(), 1);
        assert_eq!(ctx.line_text(1), Some("hello"));
    }

    #[test]
    fn test_byte_offset_to_line_at_newline_char() {
        // "hello\nworld" - offset 5 is the last byte of "hello", offset 6 is 'w'
        let ctx = LintContext::new("hello\nworld");
        // Offset 5 should be in line 1 (the 'o' at end of "hello", 0-indexed byte 5 = end of line)
        assert_eq!(ctx.byte_offset_to_line(5), Some(1));
        // Offset 6 is 'w', start of line 2
        assert_eq!(ctx.byte_offset_to_line(6), Some(2));
    }

    #[test]
    fn test_byte_offset_to_line_crlf_at_newline() {
        // "hi\r\nbye" => line 1 = "hi" (0..2), CRLF at 2..4, line 2 = "bye" (4..7)
        let ctx = LintContext::new("hi\r\nbye");
        assert_eq!(ctx.byte_offset_to_line(0), Some(1)); // 'h'
        assert_eq!(ctx.byte_offset_to_line(1), Some(1)); // 'i'
        assert_eq!(ctx.byte_offset_to_line(2), Some(1)); // \r (between line end and next line start)
        assert_eq!(ctx.byte_offset_to_line(3), Some(1)); // \n (still in the gap)
        assert_eq!(ctx.byte_offset_to_line(4), Some(2)); // 'b'
    }

    #[test]
    fn test_line_text_blank_middle_line() {
        let ctx = LintContext::new("a\n\nb");
        assert_eq!(ctx.line_text(1), Some("a"));
        assert_eq!(ctx.line_text(2), Some(""));
        assert_eq!(ctx.line_text(3), Some("b"));
    }

    #[test]
    fn test_line_text_crlf_content() {
        let ctx = LintContext::new("hello\r\nworld");
        assert_eq!(ctx.line_text(1), Some("hello"));
        assert_eq!(ctx.line_text(2), Some("world"));
    }

    #[test]
    fn test_byte_offset_to_line_at_source_end() {
        let ctx = LintContext::new("abc");
        // Offset 3 = one past last byte, should still resolve to line 1
        assert_eq!(ctx.byte_offset_to_line(3), Some(1));
        // Offset 4 = out of bounds
        assert_eq!(ctx.byte_offset_to_line(4), None);
    }

    #[test]
    fn test_source_accessor() {
        let ctx = LintContext::new("hello world");
        assert_eq!(ctx.source(), "hello world");
    }

    #[test]
    fn test_with_ast_source_accessor() {
        let arena = AstArena::new();
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 5),
            &[],
        ));
        let ctx = LintContext::with_ast("hello", doc);
        assert_eq!(ctx.source(), "hello");
        assert_eq!(ctx.line_count(), 1);
    }

    #[test]
    fn test_only_newlines() {
        let ctx = LintContext::new("\n\n");
        // "\n\n" => line 1: "", line 2: "", plus trailing blank from final \n
        assert_eq!(ctx.line_count(), 3);
        assert!(ctx.line_info(1).unwrap().is_blank);
        assert!(ctx.line_info(2).unwrap().is_blank);
        assert!(ctx.line_info(3).unwrap().is_blank);
    }

    #[test]
    fn test_extract_text_from_nested_nodes() {
        let arena = AstArena::new();
        let s1 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "Hello"));
        let s2 = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(6, 11), "World"));
        let em_children = arena.alloc_slice_copy(&[*s2]);
        let em = arena.alloc(TxtNode::new_parent(
            NodeType::Emphasis,
            Span::new(5, 12),
            em_children,
        ));
        let h_children = arena.alloc_slice_copy(&[*s1, *em]);
        let mut header = TxtNode::new_parent(NodeType::Header, Span::new(0, 12), h_children);
        header.data = NodeData::header(1);
        let header_ref = arena.alloc(header);

        let doc_children = arena.alloc_slice_copy(&[*header_ref]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 12),
            doc_children,
        ));

        let ctx = LintContext::with_ast("Hello *World*", doc);
        let structure = ctx.structure();

        assert_eq!(structure.headings.len(), 1);
        assert_eq!(structure.headings[0].text, "HelloWorld");
    }
}

#[cfg(test)]
mod tests_content_characteristics {
    use super::*;

    #[test]
    fn test_detect_headings() {
        let chars = ContentCharacteristics::analyze("# Heading\n## Sub");
        assert!(chars.has_headings);
        assert!(!chars.has_lists);
    }

    #[test]
    fn test_detect_multiple() {
        let chars = ContentCharacteristics::analyze("# Title\n\n- item 1\n- item 2\n\n[link](url)");
        assert!(chars.has_headings);
        assert!(chars.has_lists);
        assert!(chars.has_links);
    }

    #[test]
    fn test_empty_content() {
        let chars = ContentCharacteristics::analyze("");
        assert!(!chars.has_headings);
        assert!(!chars.has_lists);
    }

    #[test]
    fn test_should_skip_rule() {
        let chars = ContentCharacteristics::analyze("Just plain text");
        assert!(chars.should_skip_rule(&["Heading".to_string()]));
        assert!(!chars.should_skip_rule(&["Str".to_string()]));
    }

    #[test]
    fn test_detect_images() {
        let chars = ContentCharacteristics::analyze("![alt](image.png)");
        assert!(chars.has_images);
        assert!(chars.has_links);
    }

    #[test]
    fn test_detect_code_blocks() {
        let chars = ContentCharacteristics::analyze("```\ncode\n```");
        assert!(chars.has_fenced_code);
        assert!(!chars.has_code_blocks);
    }

    #[test]
    fn test_detect_indented_code() {
        let chars = ContentCharacteristics::analyze("    code here");
        assert!(chars.has_code_blocks);
    }

    #[test]
    fn test_detect_tables() {
        let chars = ContentCharacteristics::analyze("| a | b |\n|---|---|");
        assert!(chars.has_tables);
    }

    #[test]
    fn test_detect_blockquotes() {
        let chars = ContentCharacteristics::analyze("> quote");
        assert!(chars.has_blockquotes);
    }

    #[test]
    fn test_detect_html() {
        let chars = ContentCharacteristics::analyze("<div>content</div>");
        assert!(chars.has_html);
    }

    #[test]
    fn test_lint_context_characteristics() {
        let ctx = LintContext::new("# Title\n\n- item");
        assert!(ctx.characteristics().has_headings);
        assert!(ctx.characteristics().has_lists);
        assert!(!ctx.characteristics().has_code_blocks);
    }

    #[test]
    fn test_lint_context_should_skip_rule() {
        let ctx = LintContext::new("plain text only");
        assert!(ctx.should_skip_rule(&["Heading".to_string()]));
        assert!(ctx.should_skip_rule(&["CodeBlock".to_string()]));
        assert!(!ctx.should_skip_rule(&[]));
    }

    #[test]
    fn test_detect_inline_code() {
        let chars = ContentCharacteristics::analyze("This has `inline code` here");
        assert!(chars.has_inline_code);
        assert!(!chars.has_fenced_code);
        assert!(!chars.has_code_blocks);
    }

    #[test]
    fn test_should_skip_rule_code_vs_codeblock() {
        // Inline code only - should NOT skip "code" rules
        let chars = ContentCharacteristics::analyze("`inline`");
        assert!(!chars.should_skip_rule(&["code".to_string()]));
        assert!(chars.should_skip_rule(&["CodeBlock".to_string()]));

        // Fenced code only - should NOT skip "CodeBlock" rules
        let chars = ContentCharacteristics::analyze("```\ncode\n```");
        assert!(chars.should_skip_rule(&["code".to_string()]));
        assert!(!chars.should_skip_rule(&["CodeBlock".to_string()]));

        // Both - neither should be skipped
        let chars = ContentCharacteristics::analyze("`inline`\n\n```\ncode\n```");
        assert!(!chars.should_skip_rule(&["code".to_string()]));
        assert!(!chars.should_skip_rule(&["CodeBlock".to_string()]));
    }

    #[test]
    fn test_detect_setext_headings_h1() {
        let chars = ContentCharacteristics::analyze("My Title\n========");
        assert!(chars.has_headings);
    }

    #[test]
    fn test_detect_setext_headings_h2() {
        let chars = ContentCharacteristics::analyze("Subtitle\n--------");
        assert!(chars.has_headings);
    }

    #[test]
    fn test_detect_ordered_lists() {
        let chars = ContentCharacteristics::analyze("1. First item\n2. Second item");
        assert!(chars.has_lists);
    }

    #[test]
    fn test_detect_ordered_lists_paren() {
        let chars = ContentCharacteristics::analyze("1) First item\n2) Second item");
        assert!(chars.has_lists);
    }

    #[test]
    fn test_should_skip_rule_multiple_types() {
        let chars = ContentCharacteristics::analyze("# Heading\n\nJust text");

        // Has Heading but not List - should NOT skip if ANY type exists
        assert!(!chars.should_skip_rule(&["Heading".to_string(), "Str".to_string()]));
        assert!(!chars.should_skip_rule(&["Heading".to_string(), "List".to_string()]));

        // All absent types - should skip
        assert!(chars.should_skip_rule(&["List".to_string(), "Table".to_string()]));
    }

    #[test]
    fn test_detect_ordered_lists_multidigit() {
        let chars = ContentCharacteristics::analyze("10. Tenth item\n99. Ninety-ninth");
        assert!(
            chars.has_lists,
            "multi-digit ordered lists should be detected"
        );
    }

    #[test]
    fn test_detect_ordered_lists_multidigit_paren() {
        let chars = ContentCharacteristics::analyze("10) Tenth\n100) Hundredth");
        assert!(chars.has_lists, "multi-digit with ) should be detected");
    }

    #[test]
    fn test_should_skip_rule_code_pascalcase() {
        let chars = ContentCharacteristics::analyze("no inline code here");
        assert!(chars.should_skip_rule(&["Code".to_string()]));
        assert!(chars.should_skip_rule(&["code".to_string()]));

        let chars_with_code = ContentCharacteristics::analyze("`inline`");
        assert!(!chars_with_code.should_skip_rule(&["Code".to_string()]));
        assert!(!chars_with_code.should_skip_rule(&["code".to_string()]));
    }

    #[test]
    fn test_no_false_positive_horizontal_rule() {
        let chars = ContentCharacteristics::analyze("\n---\n");
        assert!(
            !chars.has_headings,
            "horizontal rule should not trigger has_headings"
        );
    }

    #[test]
    fn test_setext_with_preceding_content() {
        let chars = ContentCharacteristics::analyze("My Title\n========");
        assert!(
            chars.has_headings,
            "setext heading with content should be detected"
        );
    }

    #[test]
    fn test_setext_heading_no_mixed_chars() {
        // Mixed '=' and '-' is NOT a valid setext heading per CommonMark
        let chars = ContentCharacteristics::analyze("My Title\n=-=-=-=");
        assert!(
            !chars.has_headings,
            "mixed '=' and '-' should not be detected as heading"
        );
    }

    #[test]
    fn test_should_skip_rule_with_ast_canonical_names() {
        // Test that AST canonical names (Header, BlockQuote) match correctly
        let chars = ContentCharacteristics::analyze("plain text only");

        // "Header" is the AST canonical name (NodeType::Header.to_string())
        assert!(
            chars.should_skip_rule(&["Header".to_string()]),
            "Header (AST canonical) should match"
        );
        // Also accept "Heading" for convenience
        assert!(
            chars.should_skip_rule(&["Heading".to_string()]),
            "Heading should also match"
        );

        // "BlockQuote" is the AST canonical name (NodeType::BlockQuote.to_string())
        assert!(
            chars.should_skip_rule(&["BlockQuote".to_string()]),
            "BlockQuote (AST canonical) should match"
        );
        // Also accept "Blockquote" for convenience
        assert!(
            chars.should_skip_rule(&["Blockquote".to_string()]),
            "Blockquote should also match"
        );
    }
}
