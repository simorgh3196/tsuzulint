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

/// Flatten the mdast pointer tree into the contiguous index-AST.
fn transform(root: &mdast::Node, text: &str) -> Ast {
    let mut nodes: Vec<Node> = Vec::new();
    let root_span = Span::new(0, text.len() as u32);
    visit(root, NodeId(0), root_span, &mut nodes);
    Ast {
        nodes,
        text: text.to_string(),
        root: NodeId(0),
    }
}

/// Append `node` (pre-order), recurse into children, and wire up the
/// `first_child`/`next_sibling` links. Returns the new node's id.
fn visit(node: &mdast::Node, parent: NodeId, parent_span: Span, nodes: &mut Vec<Node>) -> NodeId {
    let id = NodeId(nodes.len() as u32);
    let span = span_of(node, parent_span);
    // The root's parent is itself, by the AstCoreV1 convention.
    nodes.push(Node {
        kind: map_kind(node),
        span,
        parent,
        first_child: OptionNodeId::NONE,
        next_sibling: OptionNodeId::NONE,
    });

    if let Some(children) = node.children() {
        let mut prev: Option<NodeId> = None;
        for child in children {
            let child_id = visit(child, id, span, nodes);
            match prev {
                Some(p) => nodes[p.0 as usize].next_sibling = OptionNodeId::some(child_id),
                None => nodes[id.0 as usize].first_child = OptionNodeId::some(child_id),
            }
            prev = Some(child_id);
        }
    }
    id
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
}
