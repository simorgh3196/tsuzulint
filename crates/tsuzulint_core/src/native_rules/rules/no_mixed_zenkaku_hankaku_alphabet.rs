//! Native port of `no-mixed-zenkaku-and-hankaku-alphabet`.
//!
//! Flags any Str node that contains both half-width (ASCII) alphabet
//! characters and full-width (U+FF21..U+FF5A) alphabet characters — a
//! common source of search / collation bugs in Japanese technical text.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-mixed-zenkaku-hankaku-alphabet";

fn is_halfwidth_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn is_fullwidth_alpha(c: char) -> bool {
    // U+FF21..U+FF3A (A-Z) and U+FF41..U+FF5A (a-z)
    let cp = c as u32;
    (0xFF21..=0xFF3A).contains(&cp) || (0xFF41..=0xFF5A).contains(&cp)
}

pub struct NoMixedZenkakuHankakuAlphabet;

pub static RULE: NoMixedZenkakuHankakuAlphabet = NoMixedZenkakuHankakuAlphabet;

impl Rule for NoMixedZenkakuHankakuAlphabet {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "半角英字と全角英字が混在している段落を検出する。"
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

        let mut half_pos: Option<usize> = None;
        let mut full_pos: Option<usize> = None;
        let mut byte_pos = 0usize;
        for c in text.chars() {
            let char_len = c.len_utf8();
            if is_halfwidth_alpha(c) && half_pos.is_none() {
                half_pos = Some(byte_pos);
            }
            if is_fullwidth_alpha(c) && full_pos.is_none() {
                full_pos = Some(byte_pos);
            }
            byte_pos += char_len;
            if half_pos.is_some() && full_pos.is_some() {
                break;
            }
        }

        if let (Some(h), Some(f)) = (half_pos, full_pos) {
            let first = h.min(f);
            // Report the single mixed span covering the earliest offending
            // character's single-char range; callers who want the full block
            // can look at the Str node.
            let span_start = start + first;
            let span_end = span_start + 1;
            out.push(
                Diagnostic::new(
                    RULE_ID,
                    "半角英字と全角英字が混在しています。どちらかに統一してください。",
                    Span::new(span_start as u32, span_end as u32),
                )
                .with_severity(Severity::Warning),
            );
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
        NoMixedZenkakuHankakuAlphabet.lint(&ctx)
    }

    #[test]
    fn flags_mixed() {
        // half-width "git" and full-width "Ｇｉｔ" in the same paragraph
        let diags = run("gitとＧｉｔは区別される");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn halfwidth_only_passes() {
        let diags = run("gitで管理する");
        assert!(diags.is_empty());
    }

    #[test]
    fn fullwidth_only_passes() {
        let diags = run("Ｇｉｔで管理する");
        assert!(diags.is_empty());
    }
}
