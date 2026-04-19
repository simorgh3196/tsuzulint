//! Native port of `no-zero-width-spaces`.
//!
//! Detects zero-width-ish invisible codepoints that often sneak in via
//! copy/paste from rich-text editors. Reported as warnings because they
//! are almost never intentional in technical writing.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-zero-width-spaces";

/// Characters that render as zero-width or near-zero-width in most fonts.
/// Excludes code-block-only characters (e.g. byte order marks in headers of
/// files, which are intentional).
const INVISIBLE: &[char] = &[
    '\u{200B}', // ZERO WIDTH SPACE
    '\u{200C}', // ZERO WIDTH NON-JOINER
    '\u{200D}', // ZERO WIDTH JOINER
    '\u{2060}', // WORD JOINER
    '\u{FEFF}', // ZERO WIDTH NO-BREAK SPACE (BOM)
];

pub struct NoZeroWidthSpaces;

pub static RULE: NoZeroWidthSpaces = NoZeroWidthSpaces;

impl Rule for NoZeroWidthSpaces {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Disallow invisible / zero-width Unicode codepoints that are usually unintended."
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let _ = Value::Null; // options reserved for future per-codepoint toggles
        let mut out = Vec::new();
        scan(ctx.ast, ctx.source, &mut out);
        out
    }
}

fn scan(node: &TxtNode<'_>, source: &str, out: &mut Vec<Diagnostic>) {
    // CodeBlock / Code: intentional invisibles are common (e.g. demonstrating
    // Unicode), but plain prose should not contain them.
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
            if INVISIBLE.contains(&c) {
                let abs_start = start + byte_pos;
                let abs_end = abs_start + char_len;
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        format!(
                            "Invisible codepoint U+{:04X} is not allowed in prose.",
                            c as u32
                        ),
                        Span::new(abs_start as u32, abs_end as u32),
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

    fn run(text: &str) -> Vec<Diagnostic> {
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
        NoZeroWidthSpaces.lint(&ctx)
    }

    #[test]
    fn detects_zws() {
        let diags = run("hello\u{200B}world");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("U+200B"));
    }

    #[test]
    fn clean_text_passes() {
        let diags = run("普通の文章です。");
        assert!(diags.is_empty());
    }

    #[test]
    fn detects_bom_mid_text() {
        let diags = run("foo\u{FEFF}bar");
        assert_eq!(diags.len(), 1);
    }
}
