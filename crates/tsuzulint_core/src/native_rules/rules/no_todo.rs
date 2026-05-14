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
    for (pattern_idx, pattern) in config.patterns.iter().enumerate() {
        let mut search_start = 0usize;
        while let Some((match_start, match_end)) =
            find_pattern(text, pattern, search_start, config.case_sensitive)
        {
            let matched_text = &text[match_start..match_end];

            let ignored = config.ignore_patterns.iter().any(|ignore_pat| {
                contains_pattern(matched_text, ignore_pat, config.case_sensitive)
            });

            if !ignored {
                let display = config.patterns[pattern_idx].trim_end();
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

/// Find the first occurrence of `pattern` in `haystack[start..]`, returning
/// `(match_start, match_end)` as byte offsets into the **original**
/// `haystack`.
///
/// This avoids the trap where `text.to_lowercase()` can change byte length
/// for a handful of Unicode characters (e.g. U+1E9E `ẞ` → "ss", U+0130
/// `İ` → "i\u{0307}"). A naive "lowercase then .find()" implementation
/// returns byte positions into the lowercased string which are not
/// necessarily valid in the original — slicing the original there can land
/// mid-codepoint and panic.
fn find_pattern(
    haystack: &str,
    pattern: &str,
    start: usize,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    if case_sensitive {
        let idx = haystack[start..].find(pattern)?;
        let match_start = start + idx;
        return Some((match_start, match_start + pattern.len()));
    }

    // Case-insensitive: walk the haystack character-by-character, comparing
    // against the pattern via case-insensitive char equality, while keeping
    // byte positions anchored to the ORIGINAL haystack.
    let pattern_chars: Vec<char> = pattern.chars().collect();
    if pattern_chars.is_empty() {
        return Some((start, start));
    }

    let mut char_starts: Vec<usize> = Vec::new();
    for (i, _) in haystack[start..].char_indices() {
        char_starts.push(start + i);
    }
    char_starts.push(haystack.len());

    'outer: for (i, &anchor) in char_starts[..char_starts.len().saturating_sub(1)]
        .iter()
        .enumerate()
    {
        let mut cursor = anchor;
        for &pc in &pattern_chars {
            let Some(hc) = haystack[cursor..].chars().next() else {
                continue 'outer;
            };
            if !chars_equal_ignore_case(pc, hc) {
                continue 'outer;
            }
            cursor += hc.len_utf8();
        }
        let _ = i;
        return Some((anchor, cursor));
    }
    None
}

fn contains_pattern(haystack: &str, pattern: &str, case_sensitive: bool) -> bool {
    find_pattern(haystack, pattern, 0, case_sensitive).is_some()
}

/// Case-insensitive char equality that works for the characters TODO / FIXME
/// marker users actually type. We compare by lowercasing each char into a
/// small String; this handles the special-case multi-char expansions (e.g.
/// `İ → i\u{0307}`) by treating them as non-matches for single-char
/// patterns, which is the safe default — the markers we care about are all
/// ASCII anyway.
fn chars_equal_ignore_case(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    let mut la = a.to_lowercase();
    let mut lb = b.to_lowercase();
    let (Some(ca), Some(cb)) = (la.next(), lb.next()) else {
        return false;
    };
    ca == cb && la.next().is_none() && lb.next().is_none()
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
    fn does_not_panic_on_multi_byte_lowercase_expanding_chars() {
        // Regression: `str::to_lowercase()` changes the byte length of
        // U+1E9E (`ẞ` → "ss") and U+0130 (`İ` → "i\u{0307}"), which used
        // to shift the byte index of a TODO: match into the middle of a
        // multi-byte char when indexing the original text afterwards.
        // This test exercises both "expanding" (ẞ) and "growing" (İ) forms
        // and passes only if the rule tracks byte positions in the
        // *original* text rather than the lowercased copy.
        let inputs = [
            "日ẞ日TODO: fix",  // expanding: ẞ (3B) → "ss" (2B)
            "İabcTODO: later", // growing:  İ (2B) → "i\u{0307}" (3B)
            "FOOẞBARFIXME: x",
        ];
        for text in inputs {
            let diags = run(text);
            assert!(
                !diags.is_empty(),
                "expected at least one diagnostic for {:?}",
                text
            );
            for d in &diags {
                // Spans must not panic when slicing the original text.
                let _slice = &text[d.span.start as usize..d.span.end as usize];
            }
        }
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
