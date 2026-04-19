//! Native port of `no-exclamation-question-mark`.
//!
//! Flags "!", "?", "！", "？" in prose. Technical writing style guides
//! generally discourage exclamation / question marks; this rule makes that
//! a lint error so reviewers don't have to catch it.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-exclamation-question-mark";

struct Config {
    allow_halfwidth_exclamation: bool,
    allow_fullwidth_exclamation: bool,
    allow_halfwidth_question: bool,
    allow_fullwidth_question: bool,
}

impl Config {
    fn from_options(options: &Value) -> Self {
        let mut cfg = Self {
            allow_halfwidth_exclamation: false,
            allow_fullwidth_exclamation: false,
            allow_halfwidth_question: false,
            allow_fullwidth_question: false,
        };
        let Value::Object(map) = options else {
            return cfg;
        };
        if let Some(Value::Bool(b)) = map.get("allow_halfwidth_exclamation") {
            cfg.allow_halfwidth_exclamation = *b;
        }
        if let Some(Value::Bool(b)) = map.get("allow_fullwidth_exclamation") {
            cfg.allow_fullwidth_exclamation = *b;
        }
        if let Some(Value::Bool(b)) = map.get("allow_halfwidth_question") {
            cfg.allow_halfwidth_question = *b;
        }
        if let Some(Value::Bool(b)) = map.get("allow_fullwidth_question") {
            cfg.allow_fullwidth_question = *b;
        }
        cfg
    }
}

pub struct NoExclamationQuestionMark;

pub static RULE: NoExclamationQuestionMark = NoExclamationQuestionMark;

impl Rule for NoExclamationQuestionMark {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Disallow exclamation / question marks in technical prose."
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
        for c in text.chars() {
            let char_len = c.len_utf8();
            let hit = match c {
                '!' if !config.allow_halfwidth_exclamation => Some("!"),
                '！' if !config.allow_fullwidth_exclamation => Some("！"),
                '?' if !config.allow_halfwidth_question => Some("?"),
                '？' if !config.allow_fullwidth_question => Some("？"),
                _ => None,
            };
            if let Some(mark) = hit {
                let abs_start = start + byte_pos;
                let abs_end = abs_start + char_len;
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        format!("Disallowed punctuation: '{}'.", mark),
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
        scan(child, source, config, out);
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
        let paragraph_children = arena.alloc_slice_copy(&[*s]);
        let paragraph = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            AstSpan::new(0, text.len() as u32),
            paragraph_children,
        ));
        let doc_children = arena.alloc_slice_copy(&[*paragraph]);
        let ast = TxtNode::new_parent(
            NodeType::Document,
            AstSpan::new(0, text.len() as u32),
            doc_children,
        );
        let ctx = RuleContext {
            ast: &ast,
            source: text,
            tokens: &[],
            sentences: &[],
            options: &Value::Null,
            file_path: None,
        };
        NoExclamationQuestionMark.lint(&ctx)
    }

    #[test]
    fn detects_half_width_exclamation() {
        let diags = run("hello!");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn detects_full_width_question() {
        let diags = run("ですか？");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn clean_text_passes() {
        let diags = run("これは普通の文です。");
        assert!(diags.is_empty());
    }
}
