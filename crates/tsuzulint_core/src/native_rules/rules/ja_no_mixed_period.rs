//! Native port of `ja-no-mixed-period`.
//!
//! Flags documents that use both Japanese periods (`。`) and full-stops (`.`)
//! as sentence terminators. The rule picks the dominant style (more frequent
//! terminator) and reports occurrences of the other.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "ja-no-mixed-period";

pub struct JaNoMixedPeriod;
pub static RULE: JaNoMixedPeriod = JaNoMixedPeriod;

impl Rule for JaNoMixedPeriod {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "同一ドキュメント内で句点「。」と「.」が混在することを禁止する。"
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let _ = Value::Null;
        let mut positions_period_ja: Vec<(usize, usize)> = Vec::new();
        let mut positions_period_en: Vec<(usize, usize)> = Vec::new();
        collect(
            ctx.ast,
            ctx.source,
            &mut positions_period_ja,
            &mut positions_period_en,
        );
        if positions_period_ja.is_empty() || positions_period_en.is_empty() {
            return Vec::new();
        }
        // Report the less-frequent one as the "minority" style that should be
        // normalized. When equal, we flag the English '.' (Japanese prose
        // convention in the wild).
        let (minority, minority_label, majority_label) =
            if positions_period_ja.len() < positions_period_en.len() {
                (positions_period_ja, "。", ".")
            } else {
                (positions_period_en, ".", "。")
            };
        minority
            .into_iter()
            .map(|(s, e)| {
                Diagnostic::new(
                    RULE_ID,
                    format!(
                        "句点の表記が混在しています。文書全体で多く使われている「{}」に合わせて「{}」を書き換えてください。",
                        majority_label, minority_label
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
    ja_hits: &mut Vec<(usize, usize)>,
    en_hits: &mut Vec<(usize, usize)>,
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
        let mut prev_char: Option<char> = None;
        let mut next_chars = text.chars().peekable();
        while let Some(c) = next_chars.next() {
            let char_len = c.len_utf8();
            match c {
                '。' => ja_hits.push((start + byte_pos, start + byte_pos + char_len)),
                '.' => {
                    // Only treat '.' as a sentence terminator when it is
                    // followed by whitespace, end of text, or a newline — i.e.
                    // not a decimal ("1.5") or abbreviation ("e.g.").
                    let after = next_chars.peek().copied();
                    let before = prev_char;
                    let looks_like_sentence_end = match after {
                        None => true,
                        Some(next_c) => next_c.is_whitespace() || next_c == '\n',
                    };
                    let looks_numeric = before.is_some_and(|p| p.is_ascii_digit())
                        && after.is_some_and(|a| a.is_ascii_digit());
                    if looks_like_sentence_end && !looks_numeric {
                        en_hits.push((start + byte_pos, start + byte_pos + char_len));
                    }
                }
                _ => {}
            }
            byte_pos += char_len;
            prev_char = Some(c);
        }
        return;
    }
    for child in node.children.iter() {
        collect(child, source, ja_hits, en_hits);
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
        JaNoMixedPeriod.lint(&ctx)
    }

    #[test]
    fn passes_when_only_ja() {
        assert!(lint("これは文です。そしてもう一つ。").is_empty());
    }

    #[test]
    fn passes_when_only_en() {
        assert!(lint("This is a sentence. And another.").is_empty());
    }

    #[test]
    fn flags_minority_en_period() {
        let diags = lint("これは文です。もう一つ。最後の一つだ.");
        assert_eq!(diags.len(), 1, "{:?}", diags);
    }

    #[test]
    fn ignores_decimals() {
        // "1.5" must not be treated as a sentence-terminating period.
        let diags = lint("これは文です。値は1.5です。");
        assert!(diags.is_empty());
    }
}
