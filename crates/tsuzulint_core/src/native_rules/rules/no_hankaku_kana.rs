//! Native port of `no-hankaku-kana`.
//!
//! Flags half-width katakana characters (U+FF61..U+FF9F) in prose. Full-width
//! katakana is the convention for modern Japanese technical writing.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-hankaku-kana";

fn is_halfwidth_kana(c: char) -> bool {
    let cp = c as u32;
    (0xFF61..=0xFF9F).contains(&cp)
}

pub struct NoHankakuKana;
pub static RULE: NoHankakuKana = NoHankakuKana;

impl Rule for NoHankakuKana {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "半角カタカナの使用を禁止する (全角カタカナを推奨)。"
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
        let mut run_start: Option<usize> = None;
        for c in text.chars() {
            let char_len = c.len_utf8();
            if is_halfwidth_kana(c) {
                if run_start.is_none() {
                    run_start = Some(byte_pos);
                }
            } else if let Some(rs) = run_start.take() {
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        "半角カタカナは推奨されません。全角カタカナを使ってください。",
                        Span::new((start + rs) as u32, (start + byte_pos) as u32),
                    )
                    .with_severity(Severity::Warning),
                );
            }
            byte_pos += char_len;
        }
        if let Some(rs) = run_start {
            out.push(
                Diagnostic::new(
                    RULE_ID,
                    "半角カタカナは推奨されません。全角カタカナを使ってください。",
                    Span::new((start + rs) as u32, (start + byte_pos) as u32),
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
        NoHankakuKana.lint(&ctx)
    }

    #[test]
    fn detects_halfwidth_katakana() {
        let diags = run("ｺﾝﾆﾁﾊ");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn fullwidth_passes() {
        let diags = run("コンニチハ");
        assert!(diags.is_empty());
    }
}
