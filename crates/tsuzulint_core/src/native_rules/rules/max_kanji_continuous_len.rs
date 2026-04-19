//! Native port of `max-kanji-continuous-len`.
//!
//! Flags runs of consecutive kanji characters longer than the configured
//! max length. Long kanji strings hurt readability; inserting ひらがな or
//! 送り仮名 breaks them up.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "max-kanji-continuous-len";
const DEFAULT_MAX: usize = 5;

fn is_kanji(c: char) -> bool {
    // CJK Unified Ideographs + Extension A
    let cp = c as u32;
    (0x3400..=0x4DBF).contains(&cp) || (0x4E00..=0x9FFF).contains(&cp)
}

struct Config {
    max: usize,
}

impl Config {
    fn from_options(options: &Value) -> Self {
        let mut cfg = Self { max: DEFAULT_MAX };
        if let Value::Object(map) = options
            && let Some(Value::Number(n)) = map.get("max")
            && let Some(v) = n.as_u64()
        {
            cfg.max = v as usize;
        }
        cfg
    }
}

pub struct MaxKanjiContinuousLen;
pub static RULE: MaxKanjiContinuousLen = MaxKanjiContinuousLen;

impl Rule for MaxKanjiContinuousLen {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Flag continuous kanji runs longer than the configured maximum."
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let config = Config::from_options(ctx.options);
        let mut out = Vec::new();
        scan(ctx.ast, ctx.source, &config, &mut out);
        out
    }
}

fn scan(node: &TxtNode<'_>, source: &str, config: &Config, out: &mut Vec<Diagnostic>) {
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
        let mut run_char_count = 0usize;
        for c in text.chars() {
            let char_len = c.len_utf8();
            if is_kanji(c) {
                if run_start.is_none() {
                    run_start = Some(byte_pos);
                    run_char_count = 1;
                } else {
                    run_char_count += 1;
                }
            } else if let Some(rs) = run_start.take() {
                if run_char_count > config.max {
                    out.push(
                        Diagnostic::new(
                            RULE_ID,
                            format!(
                                "Kanji run of length {} exceeds the maximum of {}.",
                                run_char_count, config.max
                            ),
                            Span::new((start + rs) as u32, (start + byte_pos) as u32),
                        )
                        .with_severity(Severity::Warning),
                    );
                }
                run_char_count = 0;
            }
            byte_pos += char_len;
        }
        if let Some(rs) = run_start
            && run_char_count > config.max
        {
            out.push(
                Diagnostic::new(
                    RULE_ID,
                    format!(
                        "Kanji run of length {} exceeds the maximum of {}.",
                        run_char_count, config.max
                    ),
                    Span::new((start + rs) as u32, (start + byte_pos) as u32),
                )
                .with_severity(Severity::Warning),
            );
        }
        return;
    }
    for child in node.children.iter() {
        scan(child, source, config, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, Span as AstSpan};

    fn lint(text: &str, max: usize) -> Vec<Diagnostic> {
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
        let options = serde_json::json!({ "max": max });
        let ctx = RuleContext {
            ast: &ast,
            source: text,
            tokens: &[],
            sentences: &[],
            options: &options,
            file_path: None,
        };
        MaxKanjiContinuousLen.lint(&ctx)
    }

    #[test]
    fn short_run_passes() {
        assert!(lint("本日は晴天", 5).is_empty());
    }

    #[test]
    fn long_run_flagged() {
        // 8 consecutive kanji, max 5 → flagged
        let diags = lint("一二三四五六七八する", 5);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn hiragana_breaks_run() {
        // two short runs, neither over the limit
        let diags = lint("一二三の四五六", 5);
        assert!(diags.is_empty());
    }
}
