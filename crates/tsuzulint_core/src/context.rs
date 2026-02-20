//! Lint context for caching parse results.
//!
//! This module provides efficient access to parsed document information,
//! avoiding redundant computations during linting.

use std::cell::OnceCell;

use tsuzulint_ast::{NodeType, Span, TxtNode};

/// Pre-computed metadata for a single line.
#[derive(Debug, Clone, Copy)]
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

        for line in source.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('#') {
                chars.has_headings = true;
            }
            // Setext-style headings: lines consisting only of = or -
            if !chars.has_headings {
                let underline_chars: Vec<char> = trimmed.chars().collect();
                if !underline_chars.is_empty()
                    && underline_chars.iter().all(|&c| c == '=' || c == '-')
                {
                    chars.has_headings = true;
                }
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
                // Ordered list: digit(s) followed by . or )
                let mut chars_iter = trimmed.chars();
                if let Some(first) = chars_iter.next()
                    && first.is_ascii_digit()
                {
                    let rest: String = chars_iter.collect();
                    if rest.starts_with('.') || rest.starts_with(')') {
                        chars.has_lists = true;
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
            "Heading" | "heading" => !self.has_headings,
            "Link" | "link" => !self.has_links,
            "Image" | "image" => !self.has_images,
            "CodeBlock" => !self.has_code_blocks && !self.has_fenced_code,
            "code" => !self.has_inline_code,
            "List" | "list" => !self.has_lists,
            "Table" | "table" => !self.has_tables,
            "Blockquote" | "blockquote" => !self.has_blockquotes,
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
            && !lines.is_empty()
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
}
