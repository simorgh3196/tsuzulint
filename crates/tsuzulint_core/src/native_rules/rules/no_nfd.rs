//! Native port of `no-nfd`.
//!
//! Flags documents containing decomposed (NFD) Unicode codepoints that
//! should be normalized to NFC. Decomposed characters often sneak in from
//! macOS filesystems or certain copy-paste sources and break search / grep.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-nfd";

/// Unicode combining marks (Mn category codepoints that are commonly used
/// to compose with a preceding base character). We detect them as a proxy
/// for "this text is likely in NFD form".
fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    // U+0300..U+036F: Combining Diacritical Marks
    // U+1AB0..U+1AFF: Combining Diacritical Marks Extended
    // U+1DC0..U+1DFF: Combining Diacritical Marks Supplement
    // U+20D0..U+20FF: Combining Diacritical Marks for Symbols
    // U+FE20..U+FE2F: Combining Half Marks
    // U+3099..U+309A: Japanese voicing mark / semi-voicing mark
    matches!(cp,
        0x0300..=0x036F
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20FF
        | 0xFE20..=0xFE2F
        | 0x3099..=0x309A
    )
}

pub struct NoNfd;
pub static RULE: NoNfd = NoNfd;

impl Rule for NoNfd {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Flag text that contains decomposed (NFD) Unicode codepoints; normalize to NFC."
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let _ = Value::Null;
        let mut out = Vec::new();
        scan(ctx.ast, ctx.source, &mut out);
        out
    }
}

fn scan(node: &TxtNode<'_>, source: &str, out: &mut Vec<Diagnostic>) {
    if matches!(node.node_type, NodeType::CodeBlock | NodeType::Code) {
        return;
    }
    if node.node_type == NodeType::Str {
        let start = node.span.start as usize;
        let end = node.span.end as usize;
        if end > source.len() || start > end {
            return;
        }
        let text = &source[start..end];
        let mut byte_pos = 0usize;
        for c in text.chars() {
            let char_len = c.len_utf8();
            if is_combining_mark(c) {
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        format!(
                            "Combining mark U+{:04X} detected; normalize text to NFC.",
                            c as u32
                        ),
                        Span::new(
                            (start + byte_pos) as u32,
                            (start + byte_pos + char_len) as u32,
                        ),
                    )
                    .with_severity(Severity::Warning),
                );
            }
            byte_pos += char_len;
        }
        return;
    }
    for child in node.children.iter() {
        scan(child, source, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, Span as AstSpan};

    fn lint(text: &str) -> Vec<Diagnostic> {
        let arena = AstArena::new();
        let s = arena.alloc(TxtNode::new_text(
            NodeType::Str,
            AstSpan::new(0, text.len() as u32),
            text,
        ));
        let pc = arena.alloc_slice_copy(&[*s]);
        let p = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            AstSpan::new(0, text.len() as u32),
            pc,
        ));
        let dc = arena.alloc_slice_copy(&[*p]);
        let ast = TxtNode::new_parent(NodeType::Document, AstSpan::new(0, text.len() as u32), dc);
        let ctx = RuleContext {
            ast: &ast,
            source: text,
            tokens: &[],
            sentences: &[],
            options: &Value::Null,
            file_path: None,
        };
        NoNfd.lint(&ctx)
    }

    #[test]
    fn detects_nfd_japanese_dakuten() {
        // NFC "が" = U+304C; NFD = U+304B (か) + U+3099 (combining dakuten)
        let diags = lint("か\u{3099}");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn nfc_passes() {
        let diags = lint("がぎぐげご");
        assert!(diags.is_empty());
    }

    #[test]
    fn detects_accented_latin_nfd() {
        // NFC "é" = U+00E9; NFD = e + U+0301
        let diags = lint("e\u{0301}xample");
        assert_eq!(diags.len(), 1);
    }
}
