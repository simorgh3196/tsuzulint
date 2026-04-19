//! Native port of the `no-todo` rule.
//!
//! Detects task markers (TODO, FIXME, HACK, XXX) in prose nodes. Mirrors the
//! semantics of `textlint-rule-no-todo` and the WASM `rules/no-todo` plugin so
//! existing configs keep working, but runs as native Rust without a WASM
//! round-trip.

use serde_json::Value;
use tsuzulint_ast::{NodeType, Span, TxtNode};
use tsuzulint_plugin::{Diagnostic, Severity};

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-todo";
const DEFAULT_PATTERNS: &[&str] = &[
    "TODO:", "TODO ", "FIXME:", "FIXME ", "XXX:", "XXX ", "HACK:",
];

struct Config {
    patterns: Vec<String>,
    ignore_patterns: Vec<String>,
    case_sensitive: bool,
}

impl Config {
    fn from_options(options: &Value) -> Self {
        let mut cfg = Self {
            patterns: DEFAULT_PATTERNS.iter().map(|s| (*s).to_string()).collect(),
            ignore_patterns: Vec::new(),
            case_sensitive: false,
        };

        let Value::Object(map) = options else {
            return cfg;
        };

        if let Some(Value::Array(arr)) = map.get("patterns") {
            let patterns: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if !patterns.is_empty() {
                cfg.patterns = patterns;
            }
        }

        if let Some(Value::Array(arr)) = map.get("ignore_patterns") {
            cfg.ignore_patterns = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
        }

        if let Some(Value::Bool(b)) = map.get("case_sensitive") {
            cfg.case_sensitive = *b;
        }

        cfg
    }
}

pub struct NoTodo;

pub static RULE: NoTodo = NoTodo;

impl Rule for NoTodo {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Disallow TODO/FIXME/HACK/XXX markers in prose."
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let config = Config::from_options(ctx.options);
        let mut diagnostics = Vec::new();
        scan(ctx.ast, ctx.source, &config, &mut diagnostics);
        diagnostics
    }
}

fn scan(node: &TxtNode<'_>, source: &str, config: &Config, out: &mut Vec<Diagnostic>) {
    // Skip code blocks and inline code: they frequently contain TODO markers
    // that are meant for the code, not the prose.
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
        report_matches(text, start as u32, config, out);
        return;
    }

    for child in node.children.iter() {
        scan(child, source, config, out);
    }
}

fn report_matches(text: &str, base_offset: u32, config: &Config, out: &mut Vec<Diagnostic>) {
    // Building the haystack once per Str node is cheaper than re-casing per
    // pattern. `to_lowercase` is O(n) but only fires when the user opted out
    // of case sensitivity (the default for textlint-rule-no-todo).
    let haystack_owned;
    let haystack: &str = if config.case_sensitive {
        text
    } else {
        haystack_owned = text.to_lowercase();
        haystack_owned.as_str()
    };

    let patterns: Vec<String> = if config.case_sensitive {
        config.patterns.clone()
    } else {
        config.patterns.iter().map(|p| p.to_lowercase()).collect()
    };
    let ignore_patterns: Vec<String> = if config.case_sensitive {
        config.ignore_patterns.clone()
    } else {
        config
            .ignore_patterns
            .iter()
            .map(|p| p.to_lowercase())
            .collect()
    };

    for (pattern, pattern_cased) in patterns.iter().zip(config.patterns.iter()) {
        let mut search_start = 0usize;
        while let Some(idx) = haystack[search_start..].find(pattern.as_str()) {
            let match_start = search_start + idx;
            let match_end = match_start + pattern.len();
            let matched_text = &text[match_start..match_end];

            if !ignore_patterns
                .iter()
                .any(|p| matched_text.to_lowercase().contains(p.as_str()))
            {
                let display = pattern_cased.trim_end();
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        format!(
                            "Found '{}' marker. Consider resolving this before committing.",
                            display
                        ),
                        Span::new(
                            base_offset + match_start as u32,
                            base_offset + match_end as u32,
                        ),
                    )
                    .with_severity(Severity::Warning),
                );
            }

            search_start = match_end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, Span as AstSpan};

    fn make_str_ast<'a>(arena: &'a AstArena, text: &'a str) -> TxtNode<'a> {
        let start = 0u32;
        let end = text.len() as u32;
        let str_node = arena.alloc(TxtNode::new_text(
            NodeType::Str,
            AstSpan::new(start, end),
            text,
        ));
        let paragraph_children = arena.alloc_slice_copy(&[*str_node]);
        let paragraph = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            AstSpan::new(start, end),
            paragraph_children,
        ));
        let doc_children = arena.alloc_slice_copy(&[*paragraph]);
        TxtNode::new_parent(NodeType::Document, AstSpan::new(start, end), doc_children)
    }

    fn run(text: &str) -> Vec<Diagnostic> {
        let arena = AstArena::new();
        let ast = make_str_ast(&arena, text);
        let ctx = RuleContext {
            ast: &ast,
            source: text,
            tokens: &[],
            sentences: &[],
            options: &Value::Null,
            file_path: None,
        };
        NoTodo.lint(&ctx)
    }

    #[test]
    fn detects_todo() {
        let diags = run("本節は後でTODO: 書き直す。");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("TODO"));
    }

    #[test]
    fn detects_fixme_case_insensitive() {
        let diags = run("fixme: check this");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn no_match_for_clean_text() {
        let diags = run("単なる普通の文である。");
        assert!(diags.is_empty());
    }

    #[test]
    fn skips_code_block() {
        let arena = AstArena::new();
        let text = "TODO: still visible\n```\nTODO: hidden\n```";
        let para_text = "TODO: still visible";
        let code_text = "TODO: hidden";
        let str_node = arena.alloc(TxtNode::new_text(
            NodeType::Str,
            AstSpan::new(0, para_text.len() as u32),
            para_text,
        ));
        let para_children = arena.alloc_slice_copy(&[*str_node]);
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            AstSpan::new(0, para_text.len() as u32),
            para_children,
        ));
        let code_start = para_text.len() as u32 + 1;
        let code_end = code_start + code_text.len() as u32;
        let code = arena.alloc(TxtNode::new_text(
            NodeType::CodeBlock,
            AstSpan::new(code_start, code_end),
            code_text,
        ));
        let doc_children = arena.alloc_slice_copy(&[*para, *code]);
        let ast = TxtNode::new_parent(NodeType::Document, AstSpan::new(0, code_end), doc_children);
        let ctx = RuleContext {
            ast: &ast,
            source: text,
            tokens: &[],
            sentences: &[],
            options: &Value::Null,
            file_path: None,
        };
        let diags = NoTodo.lint(&ctx);
        assert_eq!(diags.len(), 1, "only paragraph TODO should fire");
    }
}
