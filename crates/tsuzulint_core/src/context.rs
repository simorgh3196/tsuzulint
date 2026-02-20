//! Lint context for caching parse results.
//!
//! This module provides efficient access to parsed document information,
//! avoiding redundant computations during linting.

use std::cell::OnceCell;

use tsuzulint_ast::{NodeType, Span, TxtNode};

/// Pre-computed metadata for a single line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineInfo {
    /// Byte offset of line start (inclusive).
    pub start: u32,
    /// Byte offset of line end (exclusive, includes newline if present).
    pub end: u32,
    /// Indentation in bytes (tabs count as 1 byte).
    pub indent: u32,
    /// Whether this line contains only whitespace.
    pub is_blank: bool,
}

impl LineInfo {
    /// Creates a new LineInfo from a line's content.
    pub fn from_line(start: u32, line_text: &str) -> Self {
        let end = start + line_text.len() as u32;
        let trimmed = line_text.trim_end();
        let is_blank = trimmed.is_empty();
        // indent is in bytes (actual byte offset), not visual width
        let indent = if is_blank {
            0
        } else {
            let leading_len = line_text.len() - line_text.trim_start().len();
            leading_len as u32
        };

        Self {
            start,
            end,
            indent,
            is_blank,
        }
    }

    /// Returns the byte offset of the first non-whitespace character.
    pub fn content_start(&self) -> u32 {
        self.start + self.indent
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

/// Context for linting a document.
///
/// Provides cached access to line information and document structure,
/// avoiding redundant computations.
pub struct LintContext<'a> {
    /// The source text.
    source: &'a str,
    /// Pre-computed line information.
    lines: Vec<LineInfo>,
    /// The AST root node (if provided).
    root: Option<&'a TxtNode<'a>>,
    /// Lazily constructed document structure.
    structure: OnceCell<DocumentStructure>,
}

impl<'a> LintContext<'a> {
    /// Creates a new LintContext from source text.
    pub fn new(source: &'a str) -> Self {
        let lines = Self::compute_lines(source);
        Self {
            source,
            lines,
            root: None,
            structure: OnceCell::new(),
        }
    }

    /// Creates a new LintContext from source text and an AST root node.
    ///
    /// The root node is stored and used for lazily building the document structure
    /// via [`structure()`](Self::structure).
    pub fn with_ast(source: &'a str, root: &'a TxtNode<'a>) -> Self {
        let lines = Self::compute_lines(source);
        Self {
            source,
            lines,
            root: Some(root),
            structure: OnceCell::new(),
        }
    }

    /// Computes line information from source text.
    fn compute_lines(source: &str) -> Vec<LineInfo> {
        let mut lines = Vec::new();
        let mut offset = 0u32;

        for line in source.lines() {
            let info = LineInfo::from_line(offset, line);
            lines.push(info);
            offset = info.end;
            // Add newline bytes: check if next bytes are \r\n (CRLF) or just \n (LF)
            if offset < source.len() as u32 {
                let bytes = source.as_bytes();
                if bytes[offset as usize] == b'\r'
                    && offset + 1 < source.len() as u32
                    && bytes[offset as usize + 1] == b'\n'
                {
                    offset += 2; // CRLF
                } else if bytes[offset as usize] == b'\n' || bytes[offset as usize] == b'\r' {
                    offset += 1; // LF or CR
                }
            }
        }

        if (source.ends_with('\n') || source.ends_with('\r'))
            && !lines.is_empty()
            && offset == source.len() as u32
        {
            lines.push(LineInfo {
                start: source.len() as u32,
                end: source.len() as u32,
                indent: 0,
                is_blank: true,
            });
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
        // Use byte slicing directly - info.start and info.end are byte offsets
        let end = if info.end > info.start && info.end <= self.source.len() as u32 {
            // Check if the last character is \r (for CRLF handling) by checking bytes
            let bytes = self.source.as_bytes();
            let last_idx = (info.end - 1) as usize;
            if bytes[last_idx] == b'\r' {
                info.end - 1
            } else {
                info.end
            }
        } else {
            info.end
        };
        Some(&self.source[info.start as usize..end as usize])
    }

    /// Extracts text content from a node and its children.
    fn extract_text(node: &TxtNode<'a>) -> String {
        let mut text = String::new();
        Self::collect_text(node, &mut text);
        text
    }

    fn collect_text(node: &TxtNode<'a>, text: &mut String) {
        if let Some(v) = node.value {
            text.push_str(v);
        }
        for child in node.children {
            Self::collect_text(child, text);
        }
    }

    /// Returns the lazily-built document structure.
    ///
    /// The structure is computed once from the AST root provided via
    /// [`with_ast()`](Self::with_ast) and cached for subsequent calls.
    /// If no root was provided (created via [`new()`](Self::new)),
    /// returns an empty `DocumentStructure`.
    pub fn structure(&self) -> &DocumentStructure {
        self.structure.get_or_init(|| {
            let mut structure = DocumentStructure::default();
            if let Some(root) = self.root {
                Self::collect_structure(root, &mut structure);
            }
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
        assert_eq!(info.indent, 1); // tab is 1 byte
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
    fn test_line_info_with_cr() {
        let info = LineInfo::from_line(0, "hello\r");
        assert_eq!(info.end, 6);
        assert!(!info.is_blank);
    }

    #[test]
    fn test_multiple_tabs() {
        let info = LineInfo::from_line(0, "\t\thello");
        assert_eq!(info.indent, 2); // 2 tabs = 2 bytes
    }

    #[test]
    fn test_mixed_indent() {
        let info = LineInfo::from_line(0, "  \thello");
        assert_eq!(info.indent, 3); // 2 spaces + 1 tab = 3 bytes
    }

    #[test]
    fn test_lint_context_trailing_newline() {
        let ctx = LintContext::new("hello\n");
        assert_eq!(ctx.line_count(), 2);
        assert_eq!(ctx.line_info(1).unwrap().end, 5);
    }

    #[test]
    fn test_crlf_newlines() {
        let ctx = LintContext::new("hello\r\nworld\r\nfoo");
        assert_eq!(ctx.line_count(), 3);

        assert_eq!(ctx.line_info(1).unwrap().start, 0);
        assert_eq!(ctx.line_info(1).unwrap().end, 5);

        assert_eq!(ctx.line_info(2).unwrap().start, 7);
        assert_eq!(ctx.line_info(2).unwrap().end, 12);

        assert_eq!(ctx.line_info(3).unwrap().start, 14);
        assert_eq!(ctx.line_info(3).unwrap().end, 17);

        assert_eq!(ctx.line_text(1), Some("hello"));
        assert_eq!(ctx.line_text(2), Some("world"));
        assert_eq!(ctx.line_text(3), Some("foo"));
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
        let arena = AstArena::new();
        let h1 = make_header_node(&arena, 1, "Title", Span::new(0, 7));
        let h2 = make_header_node(&arena, 2, "Sub", Span::new(8, 13));
        let children = arena.alloc_slice_copy(&[*h1, *h2]);
        let doc = arena.alloc(TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 13),
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
        let ctx = LintContext::new("hello\r");
        assert_eq!(ctx.line_count(), 2);
        assert_eq!(ctx.line_text(1), Some("hello"));
        assert!(ctx.line_info(2).unwrap().is_blank);
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
