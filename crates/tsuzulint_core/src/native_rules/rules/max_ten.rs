//! Native port of `max-ten`.
//!
//! Reports sentences that contain more commas (読点、、) than the
//! configured maximum. Matches the public textlint-rule-max-ten defaults.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "max-ten";
const DEFAULT_MAX: usize = 3;
const DEFAULT_TOUTEN: char = '、';
const DEFAULT_KUTEN: char = '。';

struct Config {
    max: usize,
    touten: char,
    kuten: char,
}

impl Config {
    fn from_options(options: &Value) -> Self {
        let mut cfg = Self {
            max: DEFAULT_MAX,
            touten: DEFAULT_TOUTEN,
            kuten: DEFAULT_KUTEN,
        };
        let Value::Object(map) = options else {
            return cfg;
        };
        if let Some(Value::Number(n)) = map.get("max")
            && let Some(max) = n.as_u64()
        {
            cfg.max = max as usize;
        }
        if let Some(Value::String(s)) = map.get("touten")
            && let Some(c) = s.chars().next()
        {
            cfg.touten = c;
        }
        if let Some(Value::String(s)) = map.get("kuten")
            && let Some(c) = s.chars().next()
        {
            cfg.kuten = c;
        }
        cfg
    }
}

pub struct MaxTen;

pub static RULE: MaxTen = MaxTen;

impl Rule for MaxTen {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Limit the number of Japanese commas (読点) per sentence."
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let config = Config::from_options(ctx.options);
        let mut out = Vec::new();
        scan(ctx.ast, ctx.source, &config, &mut out);
        out
    }
}

fn scan(node: &TxtNode<'_>, source: &str, config: &Config, out: &mut Vec<Diagnostic>) {
    match node.node_type {
        NodeType::CodeBlock | NodeType::Code | NodeType::Html => {}
        NodeType::Paragraph
        | NodeType::Header
        | NodeType::ListItem
        | NodeType::TableCell
        | NodeType::BlockQuote => {
            check_block(node, source, config, out);
            for child in node.children.iter() {
                scan(child, source, config, out);
            }
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

    let mut sentence_start = 0usize;
    let mut ten_count = 0usize;
    let mut byte_pos = 0usize;
    for c in text.chars() {
        let char_len = c.len_utf8();
        if c == config.touten {
            ten_count += 1;
        } else if c == config.kuten {
            if ten_count > config.max {
                let abs_start = start + sentence_start;
                let abs_end = start + byte_pos + char_len;
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        format!(
                            "Sentence contains {} '{}' marks (limit is {}).",
                            ten_count, config.touten, config.max
                        ),
                        Span::new(abs_start as u32, abs_end as u32),
                    )
                    .with_severity(Severity::Warning),
                );
            }
            ten_count = 0;
            sentence_start = byte_pos + char_len;
        }
        byte_pos += char_len;
    }
    // Trailing sentence without a kuten: still check it.
    if ten_count > config.max && sentence_start < byte_pos {
        let abs_start = start + sentence_start;
        let abs_end = start + byte_pos;
        out.push(
            Diagnostic::new(
                RULE_ID,
                format!(
                    "Sentence contains {} '{}' marks (limit is {}).",
                    ten_count, config.touten, config.max
                ),
                Span::new(abs_start as u32, abs_end as u32),
            )
            .with_severity(Severity::Warning),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, Span as AstSpan};

    fn run(text: &str, max: usize) -> Vec<Diagnostic> {
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
        MaxTen.lint(&ctx)
    }

    #[test]
    fn allows_up_to_max() {
        let diags = run("あ、い、う、え。", 3);
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn flags_too_many_commas() {
        let diags = run("あ、い、う、え、お。", 3);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn per_sentence_reset() {
        // Each sentence independently under the limit
        let diags = run("あ、い、う。か、き、く。", 3);
        assert!(diags.is_empty());
    }

    #[test]
    fn flags_trailing_sentence_without_kuten() {
        let diags = run("あ、い、う、え、お", 3);
        assert_eq!(diags.len(), 1);
    }
}
