//! Native port of `no-mixed-zenkaku-and-hankaku-alphabet`.
//!
//! Flags documents that mix half-width (ASCII A-Z, a-z) and full-width
//! (U+FF21..U+FF5A) alphabet characters — a common source of search /
//! collation bugs in Japanese technical text. Checked across the whole
//! document so a stray mismatched character in a different paragraph is
//! still caught. When both widths appear, every occurrence of the rarer
//! style is reported (same pattern as `ja-no-mixed-period`).

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
        "半角英字と全角英字が文書内で混在しているかを検査する。"
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let _ = Value::Null;
        let mut half_hits: Vec<(usize, usize)> = Vec::new();
        let mut full_hits: Vec<(usize, usize)> = Vec::new();
        collect(ctx.ast, ctx.source, &mut half_hits, &mut full_hits);
        if half_hits.is_empty() || full_hits.is_empty() {
            return Vec::new();
        }
        // Report every occurrence of the minority style. When the two counts
        // are equal we treat half-width as the minority; in Japanese
        // technical writing full-width letters are the outlier worth fixing
        // in most repos.
        let (minority, majority_label) = if full_hits.len() <= half_hits.len() {
            (full_hits, "半角英字")
        } else {
            (half_hits, "全角英字")
        };
        minority
            .into_iter()
            .map(|(s, e)| {
                Diagnostic::new(
                    RULE_ID,
                    format!(
                        "半角英字と全角英字が混在しています。文書全体で多く使われている{}に統一してください。",
                        majority_label
                    ),
                    Span::new(s as u32, e as u32),
                )
                .with_severity(Severity::Warning)
            })
            .collect()
    }
}

fn collect(
    node: &TxtNode<'_>,
    source: &str,
    half_hits: &mut Vec<(usize, usize)>,
    full_hits: &mut Vec<(usize, usize)>,
) {
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
            if is_halfwidth_alpha(c) {
                half_hits.push((start + byte_pos, start + byte_pos + char_len));
            } else if is_fullwidth_alpha(c) {
                full_hits.push((start + byte_pos, start + byte_pos + char_len));
            }
            byte_pos += char_len;
        }
        return;
    }
    for child in node.children.iter() {
        collect(child, source, half_hits, full_hits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, Span as AstSpan};

    /// Helper: wrap `text` in a single-paragraph document.
    fn one_paragraph<'a>(arena: &'a AstArena, text: &'a str) -> TxtNode<'a> {
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
        TxtNode::new_parent(NodeType::Document, AstSpan::new(0, text.len() as u32), dc)
    }

    /// Helper: wrap each of `texts` in its own paragraph so mixing happens
    /// across paragraph boundaries rather than inside a single Str node.
    fn multi_paragraph<'a>(arena: &'a AstArena, source: &'a str, texts: &[&'a str]) -> TxtNode<'a> {
        let mut cursor = 0usize;
        let mut paragraphs = Vec::with_capacity(texts.len());
        for text in texts {
            // Advance past any whitespace/separator bytes in `source`.
            while cursor < source.len() && !source[cursor..].starts_with(*text) {
                if let Some(ch) = source[cursor..].chars().next() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
            let start = cursor;
            let end = start + text.len();
            cursor = end;

            let s = arena.alloc(TxtNode::new_text(
                NodeType::Str,
                AstSpan::new(start as u32, end as u32),
                text,
            ));
            let pc = arena.alloc_slice_copy(&[*s]);
            paragraphs.push(*arena.alloc(TxtNode::new_parent(
                NodeType::Paragraph,
                AstSpan::new(start as u32, end as u32),
                pc,
            )));
        }
        let dc = arena.alloc_slice_copy(&paragraphs);
        TxtNode::new_parent(NodeType::Document, AstSpan::new(0, source.len() as u32), dc)
    }

    fn run(source: &str, ast: TxtNode<'_>) -> Vec<Diagnostic> {
        let ctx = RuleContext {
            ast: &ast,
            source,
            tokens: &[],
            sentences: &[],
            options: &Value::Null,
            file_path: None,
        };
        NoMixedZenkakuHankakuAlphabet.lint(&ctx)
    }

    #[test]
    fn flags_minority_fullwidth_in_same_paragraph() {
        // 3 half-width letters (git) vs 3 full-width letters (Ｇｉｔ).
        // Tied counts → full-width labeled minority; 3 flags.
        let arena = AstArena::new();
        let text = "gitとＧｉｔは区別される";
        let ast = one_paragraph(&arena, text);
        let diags = run(text, ast);
        assert_eq!(diags.len(), 3, "{:?}", diags);
    }

    #[test]
    fn flags_minority_across_paragraphs() {
        // First paragraph: half-width only. Second: single full-width letter.
        // Without cross-paragraph checking this would pass; with it we catch
        // the stray full-width letter.
        let arena = AstArena::new();
        let source = "git で管理する\n\nＡ だけ全角";
        let ast = multi_paragraph(&arena, source, &["git で管理する", "Ａ だけ全角"]);
        let diags = run(source, ast);
        assert_eq!(diags.len(), 1, "only the single full-width Ａ is flagged");
    }

    #[test]
    fn halfwidth_only_passes() {
        let arena = AstArena::new();
        let text = "gitで管理する";
        let ast = one_paragraph(&arena, text);
        assert!(run(text, ast).is_empty());
    }

    #[test]
    fn fullwidth_only_passes() {
        let arena = AstArena::new();
        let text = "Ｇｉｔで管理する";
        let ast = one_paragraph(&arena, text);
        assert!(run(text, ast).is_empty());
    }

    #[test]
    fn single_minority_across_the_file_is_flagged() {
        // 5 half-width letters spread across paragraphs, plus exactly one
        // stray full-width letter in the last paragraph. The full-width is
        // the unambiguous minority and must be reported.
        let arena = AstArena::new();
        let source = "git は小文字\n\nGitHub にも半角\n\n誤植の Ａ";
        let ast = multi_paragraph(
            &arena,
            source,
            &["git は小文字", "GitHub にも半角", "誤植の Ａ"],
        );
        let diags = run(source, ast);
        assert_eq!(diags.len(), 1);
    }
}
