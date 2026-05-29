//! markdown-rs parsing and the **mdast → frozen index-AST** transform.
//!
//! [`parse`] runs the markdown-rs parser and flattens its pointer tree into the contiguous
//! [`Ast`] from `tzlint_ast`: pre-order [`NodeId`]s, `parent`/`first_child`/`next_sibling`
//! links, and absolute byte [`Span`]s. CommonMark + GFM + frontmatter are enabled; MDX and
//! math are not (the `NodeKind`s for them exist, so enabling them later stays lossless).

use markdown::{ParseOptions, mdast, to_mdast};
use tzlint_ast::{Ast, Node, NodeId, NodeKind, OptionNodeId, Span};

/// A hard parse failure.
///
/// markdown is error-tolerant by construction (CommonMark always yields a tree), so this
/// is rare. The engine turns it into a single diagnostic and lints nothing else for the
/// file, rather than panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Human-readable reason, as reported by markdown-rs.
    pub message: String,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "parse error: {}", self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parse markdown `source` into the frozen index-[`Ast`].
///
/// A leading UTF-8 BOM is stripped; the returned `Ast.text` is the BOM-stripped source and
/// every [`Span`] is a byte offset into it (so spans match what the user sees). The root of
/// the tree is always [`NodeId(0)`](NodeId), a `Root` node spanning the whole document.
pub fn parse(source: &str) -> Result<Ast, ParseError> {
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);
    // Spans are u32 byte offsets; a >= 4 GiB source would truncate them. Reject rather than
    // silently emit wrong spans. (A smaller MAX_FILE cap arrives with the io layer.)
    if text.len() > u32::MAX as usize {
        return Err(ParseError {
            message: "source exceeds the 4 GiB span-offset limit".to_string(),
        });
    }
    // markdown-rs's `to_mdast` recurses on block-container nesting and *aborts* (stack
    // overflow, which is uncatchable) on pathological input like `"> ".repeat(5000)` — only
    // ~10 KB, so a byte-size cap would not catch it. Reject excessive nesting up front so it
    // degrades to a `ParseError`, never an abort. (Our own transform is iterative.)
    if max_container_marker_depth(text) > MAX_NESTING_DEPTH {
        return Err(ParseError {
            message: "input nests block containers too deeply".to_string(),
        });
    }
    let root = to_mdast(text, &parse_options()).map_err(|message| ParseError {
        message: message.to_string(),
    })?;
    Ok(transform(&root, text))
}

/// CommonMark + GFM + frontmatter (YAML/TOML). MDX and math stay off.
fn parse_options() -> ParseOptions {
    let mut options = ParseOptions::gfm();
    options.constructs.frontmatter = true;
    options
}

/// Maximum block-container nesting depth accepted by [`parse`]. Far below the recursive
/// markdown-rs parser's stack-overflow threshold, and far above any real document (which
/// nests only a handful of levels).
const MAX_NESTING_DEPTH: usize = 1000;

/// A cheap, conservative upper bound on block-container nesting: the most container
/// "openers" (`>` blockquote markers and `-`/`+`/`*`/`N.`/`N)` list bullets) leading any
/// single line. A `"> ".repeat(n)` or `"- ".repeat(n)` line nests `n` deep in only ~2n
/// bytes, so this guards the cheap-bytes / deep-nesting vector that a byte-size cap cannot.
/// It only over-counts on already-pathological lines, so it never rejects real prose.
fn max_container_marker_depth(source: &str) -> usize {
    let mut max = 0usize;
    for line in source.lines() {
        let bytes = line.as_bytes();
        let mut depth = 0usize;
        let mut i = 0usize;
        loop {
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let b = bytes[i];
            if b == b'>' {
                depth += 1;
                i += 1;
            } else if matches!(b, b'-' | b'+' | b'*')
                && i + 1 < bytes.len()
                && (bytes[i + 1] == b' ' || bytes[i + 1] == b'\t')
            {
                depth += 1;
                i += 2;
            } else if b.is_ascii_digit() {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j + 1 < bytes.len()
                    && matches!(bytes[j], b'.' | b')')
                    && (bytes[j + 1] == b' ' || bytes[j + 1] == b'\t')
                {
                    depth += 1;
                    i = j + 2;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        max = max.max(depth);
        if max > MAX_NESTING_DEPTH {
            return max; // early-out: no need to scan the rest
        }
    }
    max
}

/// Map an mdast node to its frozen [`NodeKind`].
///
/// Exhaustive **by design**: if a future markdown-rs adds a node type, this stops
/// compiling until a `NodeKind` is assigned — keeping the transform lossless over the
/// entire mdast vocabulary.
fn map_kind(node: &mdast::Node) -> NodeKind {
    use mdast::Node::*;
    match node {
        Root(_) => NodeKind::ROOT,
        Paragraph(_) => NodeKind::PARAGRAPH,
        Heading(_) => NodeKind::HEADING,
        ThematicBreak(_) => NodeKind::THEMATIC_BREAK,
        Blockquote(_) => NodeKind::BLOCKQUOTE,
        List(_) => NodeKind::LIST,
        ListItem(_) => NodeKind::LIST_ITEM,
        Code(_) => NodeKind::CODE,
        Html(_) => NodeKind::HTML,
        Definition(_) => NodeKind::DEFINITION,
        FootnoteDefinition(_) => NodeKind::FOOTNOTE_DEFINITION,
        Table(_) => NodeKind::TABLE,
        TableRow(_) => NodeKind::TABLE_ROW,
        TableCell(_) => NodeKind::TABLE_CELL,
        Yaml(_) => NodeKind::YAML,
        Toml(_) => NodeKind::TOML,
        Math(_) => NodeKind::MATH,
        Text(_) => NodeKind::TEXT,
        Emphasis(_) => NodeKind::EMPHASIS,
        Strong(_) => NodeKind::STRONG,
        Delete(_) => NodeKind::DELETE,
        InlineCode(_) => NodeKind::INLINE_CODE,
        InlineMath(_) => NodeKind::INLINE_MATH,
        Break(_) => NodeKind::BREAK,
        Link(_) => NodeKind::LINK,
        LinkReference(_) => NodeKind::LINK_REFERENCE,
        Image(_) => NodeKind::IMAGE,
        ImageReference(_) => NodeKind::IMAGE_REFERENCE,
        FootnoteReference(_) => NodeKind::FOOTNOTE_REFERENCE,
        MdxFlowExpression(_) => NodeKind::MDX_FLOW_EXPRESSION,
        MdxTextExpression(_) => NodeKind::MDX_TEXT_EXPRESSION,
        MdxJsxFlowElement(_) => NodeKind::MDX_JSX_FLOW_ELEMENT,
        MdxJsxTextElement(_) => NodeKind::MDX_JSX_TEXT_ELEMENT,
        MdxjsEsm(_) => NodeKind::MDXJS_ESM,
    }
}

/// Absolute span from an mdast node's position; falls back to `fallback` (the parent's
/// span) for the rare node that carries no position.
fn span_of(node: &mdast::Node, fallback: Span) -> Span {
    match node.position() {
        Some(p) => Span::new(p.start.offset as u32, p.end.offset as u32),
        None => fallback,
    }
}

/// Flatten the mdast pointer tree into the contiguous index-AST (pre-order ids,
/// `parent`/`first_child`/`next_sibling` links, absolute spans).
///
/// **Iterative** (explicit work stack) so a deeply nested tree cannot overflow the stack
/// here; [`parse`] separately bounds nesting so markdown-rs's own recursion can't abort
/// before this runs.
fn transform(root: &mdast::Node, text: &str) -> Ast {
    let mut nodes: Vec<Node> = Vec::new();
    let root_span = Span::new(0, text.len() as u32);
    // The root's parent is itself, by the AstCoreV1 convention.
    nodes.push(Node {
        kind: map_kind(root),
        span: span_of(root, root_span),
        parent: NodeId(0),
        first_child: OptionNodeId::NONE,
        next_sibling: OptionNodeId::NONE,
    });

    // One frame per open node: iterate its children, remembering that node's id/span and the
    // previous child's id (to chain `next_sibling`).
    struct Frame<'a> {
        children: core::slice::Iter<'a, mdast::Node>,
        parent: NodeId,
        parent_span: Span,
        last_child: Option<NodeId>,
    }

    let mut stack: Vec<Frame> = Vec::new();
    if let Some(children) = root.children() {
        stack.push(Frame {
            children: children.iter(),
            parent: NodeId(0),
            parent_span: nodes[0].span,
            last_child: None,
        });
    }

    loop {
        // Pull the next child off the top frame without holding the stack borrow across the
        // `stack.push` below (the yielded `child` borrows the mdast tree, not the stack).
        let (child, parent, parent_span, prev) = match stack.last_mut() {
            None => break,
            Some(frame) => match frame.children.next() {
                None => {
                    stack.pop();
                    continue;
                }
                Some(child) => (child, frame.parent, frame.parent_span, frame.last_child),
            },
        };

        let id = NodeId(nodes.len() as u32);
        let span = span_of(child, parent_span);
        nodes.push(Node {
            kind: map_kind(child),
            span,
            parent,
            first_child: OptionNodeId::NONE,
            next_sibling: OptionNodeId::NONE,
        });
        match prev {
            Some(p) => nodes[p.0 as usize].next_sibling = OptionNodeId::some(id),
            None => nodes[parent.0 as usize].first_child = OptionNodeId::some(id),
        }
        if let Some(frame) = stack.last_mut() {
            frame.last_child = Some(id);
        }
        if let Some(grandchildren) = child.children() {
            stack.push(Frame {
                children: grandchildren.iter(),
                parent: id,
                parent_span: span,
                last_child: None,
            });
        }
    }

    Ast {
        nodes,
        text: text.to_string(),
        root: NodeId(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect node kinds in pre-order (the storage order).
    fn kinds(ast: &Ast) -> Vec<NodeKind> {
        ast.nodes.iter().map(|n| n.kind).collect()
    }

    /// The source text covered by a node's span.
    fn text_of(ast: &Ast, id: NodeId) -> &str {
        let span = ast.nodes[id.0 as usize].span;
        &ast.text[span.start as usize..span.end as usize]
    }

    #[test]
    fn parses_simple_paragraph() {
        let ast = parse("Hello").unwrap();
        assert_eq!(ast.text, "Hello");
        assert_eq!(ast.root, NodeId(0));
        // Root -> Paragraph -> Text
        assert_eq!(
            kinds(&ast),
            vec![NodeKind::ROOT, NodeKind::PARAGRAPH, NodeKind::TEXT]
        );
        let root = &ast.nodes[0];
        assert_eq!(root.parent, NodeId(0)); // root's parent is itself
        assert_eq!(root.first_child, OptionNodeId::some(NodeId(1)));
        let para = &ast.nodes[1];
        assert_eq!(para.parent, NodeId(0));
        assert_eq!(para.first_child, OptionNodeId::some(NodeId(2)));
        assert_eq!(text_of(&ast, NodeId(2)), "Hello");
    }

    #[test]
    fn strips_leading_bom_and_keeps_offsets_aligned() {
        let ast = parse("\u{feff}# Title").unwrap();
        assert_eq!(ast.text, "# Title"); // BOM gone
        assert!(kinds(&ast).contains(&NodeKind::HEADING));
        // The heading's span indexes the BOM-stripped text.
        let heading = ast
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::HEADING)
            .unwrap();
        assert_eq!(
            &ast.text[heading.span.start as usize..heading.span.end as usize],
            "# Title"
        );
    }

    #[test]
    fn maps_common_inline_and_block_kinds() {
        let src = "# H\n\nA *em* and **strong** and `code` and [x](https://e.x).\n\n> quote\n\n- item\n\n```\nfenced\n```\n";
        let ast = parse(src).unwrap();
        let ks = kinds(&ast);
        for expected in [
            NodeKind::HEADING,
            NodeKind::PARAGRAPH,
            NodeKind::EMPHASIS,
            NodeKind::STRONG,
            NodeKind::INLINE_CODE,
            NodeKind::LINK,
            NodeKind::BLOCKQUOTE,
            NodeKind::LIST,
            NodeKind::LIST_ITEM,
            NodeKind::CODE,
            NodeKind::TEXT,
        ] {
            assert!(
                ks.contains(&expected),
                "missing kind {expected:?} in {ks:?}"
            );
        }
    }

    #[test]
    fn maps_gfm_table_and_strikethrough() {
        let src = "| a | b |\n| - | - |\n| 1 | 2 |\n\n~~gone~~\n";
        let ks = kinds(&parse(src).unwrap());
        for expected in [
            NodeKind::TABLE,
            NodeKind::TABLE_ROW,
            NodeKind::TABLE_CELL,
            NodeKind::DELETE,
        ] {
            assert!(
                ks.contains(&expected),
                "missing GFM kind {expected:?} in {ks:?}"
            );
        }
    }

    #[test]
    fn parses_yaml_frontmatter() {
        let src = "---\ntitle: hi\n---\n\n# Body\n";
        let ks = kinds(&parse(src).unwrap());
        assert!(
            ks.contains(&NodeKind::YAML),
            "frontmatter not parsed: {ks:?}"
        );
        assert!(ks.contains(&NodeKind::HEADING));
    }

    #[test]
    fn empty_input_yields_lone_root() {
        let ast = parse("").unwrap();
        assert_eq!(ast.text, "");
        assert_eq!(kinds(&ast), vec![NodeKind::ROOT]);
        assert_eq!(ast.nodes[0].first_child, OptionNodeId::NONE);
        assert_eq!(ast.nodes[0].span, Span::new(0, 0));
    }

    #[test]
    fn spans_are_byte_accurate_over_cjk() {
        let ast = parse("これは*強調*です").unwrap();
        // The emphasis node should slice to exactly "*強調*".
        let em = ast
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::EMPHASIS)
            .unwrap();
        assert_eq!(
            &ast.text[em.span.start as usize..em.span.end as usize],
            "*強調*"
        );
        // Its child Text slices to "強調".
        let child = em.first_child.get().unwrap();
        assert_eq!(text_of(&ast, child), "強調");
    }

    #[test]
    fn sibling_links_chain_in_order() {
        // Two list items: root -> list -> item0 -> ... ; item0.next_sibling -> item1.
        let ast = parse("- one\n- two\n").unwrap();
        let list = ast
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::LIST)
            .unwrap();
        let first_item = ast.nodes[list].first_child.get().unwrap();
        assert_eq!(ast.nodes[first_item.0 as usize].kind, NodeKind::LIST_ITEM);
        let second_item = ast.nodes[first_item.0 as usize].next_sibling.get().unwrap();
        assert_eq!(ast.nodes[second_item.0 as usize].kind, NodeKind::LIST_ITEM);
        // Exactly two items: the second has no further sibling.
        assert_eq!(
            ast.nodes[second_item.0 as usize].next_sibling,
            OptionNodeId::NONE
        );
    }

    #[test]
    fn every_node_parent_points_to_a_real_node() {
        let ast = parse("# H\n\ntext with **bold**\n\n- a\n- b\n").unwrap();
        for (i, node) in ast.nodes.iter().enumerate() {
            assert!(
                (node.parent.0 as usize) < ast.nodes.len(),
                "node {i} has dangling parent"
            );
            // Only the root may be its own parent.
            if i != 0 {
                assert_ne!(
                    node.parent,
                    NodeId(i as u32),
                    "non-root node {i} is its own parent"
                );
            }
        }
    }

    #[test]
    fn deeply_nested_input_is_rejected_not_aborted() {
        // ~10 KB each, but thousands of nesting levels — would stack-overflow (abort)
        // markdown-rs's recursive parser. parse() must reject these as a recoverable Err.
        for bomb in [
            "> ".repeat(5000) + "x\n",  // blockquotes
            "- ".repeat(5000) + "x\n",  // unordered lists
            "1. ".repeat(3000) + "x\n", // ordered lists
        ] {
            assert!(parse(&bomb).is_err(), "deep nesting should be a ParseError");
        }
    }

    #[test]
    fn moderate_nesting_parses_iteratively() {
        // 500 levels: under MAX_NESTING_DEPTH and within markdown-rs's own limit, so it
        // parses — and the iterative transform reproduces every level without overflowing.
        let src = "> ".repeat(500) + "deep\n";
        let ast = parse(&src).unwrap();
        let blockquotes = ast
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::BLOCKQUOTE)
            .count();
        assert_eq!(blockquotes, 500);
        assert!(kinds(&ast).contains(&NodeKind::TEXT));
    }

    #[test]
    fn nested_blockquote_contains_list() {
        // `> - a` → Blockquote → List → ListItem (container nesting links via first_child).
        let ast = parse("> - a\n").unwrap();
        let bq = ast
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::BLOCKQUOTE)
            .unwrap();
        let list = ast.nodes[bq].first_child.get().unwrap();
        assert_eq!(ast.nodes[list.0 as usize].kind, NodeKind::LIST);
        let item = ast.nodes[list.0 as usize].first_child.get().unwrap();
        assert_eq!(ast.nodes[item.0 as usize].kind, NodeKind::LIST_ITEM);
        assert_eq!(ast.nodes[item.0 as usize].parent, list);
    }

    #[test]
    fn multiple_top_level_blocks_chain_as_siblings() {
        let ast = parse("# H\n\npara\n\n- x\n").unwrap();
        let first = ast.nodes[0].first_child.get().unwrap();
        assert_eq!(ast.nodes[first.0 as usize].kind, NodeKind::HEADING);
        let second = ast.nodes[first.0 as usize].next_sibling.get().unwrap();
        assert_eq!(ast.nodes[second.0 as usize].kind, NodeKind::PARAGRAPH);
        let third = ast.nodes[second.0 as usize].next_sibling.get().unwrap();
        assert_eq!(ast.nodes[third.0 as usize].kind, NodeKind::LIST);
        assert_eq!(ast.nodes[third.0 as usize].next_sibling, OptionNodeId::NONE);
    }

    #[test]
    fn hard_line_break_is_a_break_node() {
        // A backslash at end of line is a hard break.
        let ast = parse("foo\\\nbar\n").unwrap();
        assert!(kinds(&ast).contains(&NodeKind::BREAK));
    }
}
