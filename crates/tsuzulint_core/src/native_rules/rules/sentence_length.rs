//! Native port of `sentence-length`.
//!
//! Flags sentences whose character count exceeds the configured `max`.
//! Matches the semantics of `textlint-rule-sentence-length` and the
//! `rules/sentence-length` WASM plugin.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "sentence-length";
const DEFAULT_MAX: usize = 100;

struct Config {
    max: usize,
    skip_urls: bool,
}

impl Config {
    fn from_options(options: &Value) -> Self {
        let mut cfg = Self {
            max: DEFAULT_MAX,
            skip_urls: true,
        };
        let Value::Object(map) = options else {
            return cfg;
        };
        if let Some(Value::Number(n)) = map.get("max")
            && let Some(max) = n.as_u64()
        {
            cfg.max = max as usize;
        }
        if let Some(Value::Bool(b)) = map.get("skip_urls") {
            cfg.skip_urls = *b;
        }
        cfg
    }
}

pub struct SentenceLength;

pub static RULE: SentenceLength = SentenceLength;

impl Rule for SentenceLength {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Report sentences longer than the configured maximum length."
    }

    fn needs_sentences(&self) -> bool {
        // We run per-text-node sentence splitting rather than the global
        // host splitter because individual paragraphs are the natural unit
        // (per-block diagnostics scope cleanly this way) and the per-node
        // call is already fast. If that changes later we'll flip this to
        // `true` and read from `ctx.sentences`.
        false
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let config = Config::from_options(ctx.options);
        let mut diagnostics = Vec::new();
        scan(ctx.ast, ctx.source, &config, &mut diagnostics);
        diagnostics
    }
}

/// Scan the AST for prose blocks and check each one's sentence lengths.
///
/// We operate at the *leaf* block level (Paragraph / Header / TableCell)
/// rather than container blocks (ListItem / BlockQuote / Document) because
/// list items and block quotes wrap paragraphs, and counting them as their
/// own block produces duplicate diagnostics for the same underlying text.
/// Container blocks are still traversed — we just don't run the sentence
/// check on them directly.
fn scan(node: &TxtNode<'_>, source: &str, config: &Config, out: &mut Vec<Diagnostic>) {
    match node.node_type {
        NodeType::CodeBlock | NodeType::Code | NodeType::Html | NodeType::HorizontalRule => {}
        NodeType::Paragraph | NodeType::Header | NodeType::TableCell => {
            check_block(node, source, config, out);
        }
        _ => {
            for child in node.children.iter() {
                scan(child, source, config, out);
            }
        }
    }
}

fn check_block(node: &TxtNode<'_>, source: &str, config: &Config, out: &mut Vec<Diagnostic>) {
    let start = node.span.start as usize;
    let end = node.span.end as usize;
    if end > source.len() || start > end {
        return;
    }
    let text = &source[start..end];
    // We strip URLs from the whole block BEFORE splitting, because dots inside
    // URLs would otherwise split "example.com" as two sentences. We keep a
    // parallel copy of the original sentence byte ranges so diagnostic spans
    // still point at real source offsets.
    let working_text: String = if config.skip_urls {
        strip_urls(text)
    } else {
        text.to_string()
    };
    for (sentence_text, _sentence_start) in split_sentences(&working_text) {
        let char_count = sentence_text.chars().count();
        if char_count > config.max {
            // Point the diagnostic at the block's overall span: sentence-level
            // positions would need to be reconciled with the stripped-URL
            // offsets, which adds complexity for little reader benefit.
            out.push(
                Diagnostic::new(
                    RULE_ID,
                    format!(
                        "Sentence is too long ({} characters). Maximum allowed is {}.",
                        char_count, config.max
                    ),
                    Span::new(start as u32, end as u32),
                )
                .with_severity(Severity::Warning),
            );
        }
    }
}

fn split_sentences(text: &str) -> Vec<(&str, usize)> {
    let delimiters = ['.', '!', '?', '。', '！', '？'];
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut byte_pos = 0usize;
    for c in text.chars() {
        let char_len = c.len_utf8();
        if delimiters.contains(&c) {
            let sentence_end = byte_pos + char_len;
            let sentence = text[start..sentence_end].trim_start();
            if !sentence.is_empty() {
                let leading_ws = text[start..sentence_end].len() - sentence.len();
                out.push((sentence, start + leading_ws));
            }
            start = sentence_end;
        }
        byte_pos += char_len;
    }
    if start < text.len() {
        let tail = text[start..].trim_start();
        if !tail.is_empty() {
            let leading_ws = text.len() - start - tail.len();
            out.push((tail, start + leading_ws));
        }
    }
    out
}

fn strip_urls(text: &str) -> String {
    // Replace http(s)://… URL runs with a single placeholder char so their
    // character cost collapses to 1. Anything up to the next whitespace or
    // closing punctuation is considered part of the URL.
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        let Some(rest) = text.get(i..) else {
            break;
        };
        if rest.starts_with("http://") || rest.starts_with("https://") {
            out.push('・');
            let url_end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '」' | '）' | ')' | '、' | '。'))
                .unwrap_or(rest.len());
            i += url_end;
            continue;
        }
        let ch = rest.chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, Span as AstSpan};

    fn run(source: &str, max: usize) -> Vec<Diagnostic> {
        let arena = AstArena::new();
        let str_node = arena.alloc(TxtNode::new_text(
            NodeType::Str,
            AstSpan::new(0, source.len() as u32),
            source,
        ));
        let para_children = arena.alloc_slice_copy(&[*str_node]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            AstSpan::new(0, source.len() as u32),
            para_children,
        ));
        let doc_children = arena.alloc_slice_copy(&[*para]);
        let ast = TxtNode::new_parent(
            NodeType::Document,
            AstSpan::new(0, source.len() as u32),
            doc_children,
        );
        let options = serde_json::json!({ "max": max });
        let ctx = RuleContext {
            ast: &ast,
            source,
            tokens: &[],
            sentences: &[],
            options: &options,
            file_path: None,
        };
        SentenceLength.lint(&ctx)
    }

    #[test]
    fn sentence_at_exact_limit_passes() {
        // 99 "あ" + "。" = 100 chars — exactly at the limit, must pass.
        let text = "あ".repeat(99) + "。";
        let diags = run(&text, 100);
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn sentence_one_char_over_limit_fails() {
        // 100 "あ" + "。" = 101 chars — one char over the limit.
        let text = "あ".repeat(100) + "。";
        let diags = run(&text, 100);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn only_over_limit_sentence_is_flagged() {
        // First sentence is at the limit (passes); second sentence is one
        // char over (fails). Verifies per-sentence evaluation.
        let text = format!("{}。{}。", "あ".repeat(99), "い".repeat(100));
        let diags = run(&text, 100);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn url_is_collapsed_when_skip_urls() {
        // The text without the URL is well under 100 chars; with the URL
        // counted verbatim it would exceed the limit. Default skip_urls=true
        // collapses the URL so the diagnostic does not fire.
        let text = format!(
            "詳細は {} を参照してください。",
            "https://example.com/".to_string() + &"a".repeat(200)
        );
        let diags = run(&text, 30);
        assert!(diags.is_empty(), "URL should be collapsed: {:?}", diags);
    }

    #[test]
    fn nested_list_item_paragraph_is_not_double_counted() {
        // Regression: ListItem containing a Paragraph would previously be
        // checked at both the ListItem *and* the Paragraph level, producing
        // two diagnostics (with overlapping spans) for the same sentence.
        let arena = AstArena::new();
        let source = "あ".repeat(101) + "。";
        let str_node = arena.alloc(TxtNode::new_text(
            NodeType::Str,
            AstSpan::new(0, source.len() as u32),
            &source,
        ));
        let paragraph_children = arena.alloc_slice_copy(&[*str_node]);
        let paragraph = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            AstSpan::new(0, source.len() as u32),
            paragraph_children,
        ));
        let list_item_children = arena.alloc_slice_copy(&[*paragraph]);
        let list_item = arena.alloc(TxtNode::new_parent(
            NodeType::ListItem,
            AstSpan::new(0, source.len() as u32),
            list_item_children,
        ));
        let doc_children = arena.alloc_slice_copy(&[*list_item]);
        let ast = TxtNode::new_parent(
            NodeType::Document,
            AstSpan::new(0, source.len() as u32),
            doc_children,
        );
        let options = serde_json::json!({ "max": 100 });
        let ctx = RuleContext {
            ast: &ast,
            source: &source,
            tokens: &[],
            sentences: &[],
            options: &options,
            file_path: None,
        };
        let diags = SentenceLength.lint(&ctx);
        assert_eq!(diags.len(), 1, "{:?}", diags);
    }
}
