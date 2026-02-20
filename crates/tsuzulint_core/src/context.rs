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
            if offset < self.lines[0].start {
                return None;
            }
            return Some(1);
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

    /// Builds document structure from an AST node.
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
                let depth = node.data.depth.unwrap_or(1);
                let text = Self::extract_text(node);
                structure.headings.push(HeadingInfo {
                    depth,
                    span: node.span,
                    text,
                });
            }
            NodeType::Link | NodeType::Image => {
                let url = node.data.url.unwrap_or("").to_string();
                let title = node.data.title.map(|s| s.to_string());
                structure.links.push(LinkInfo {
                    url,
                    title,
                    span: node.span,
                    is_image: node.node_type == NodeType::Image,
                });
            }
            NodeType::CodeBlock => {
                let lang = node.data.lang.map(|s| s.to_string());
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
}
