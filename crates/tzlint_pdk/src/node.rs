//! [`NodeRef`] — an ergonomic, zero-copy cursor over the archived AST.

use tzlint_ast::{ArchivedAst, ArchivedNode, NodeId, NodeKind, Span};

/// A lightweight, copyable cursor into an [`ArchivedAst`].
///
/// Every accessor reads the archived data **in place** — no allocation and no per-node
/// deserialize (which is forbidden in the hot path). Navigation follows the frozen
/// `first_child` / `next_sibling` / `parent` links.
#[derive(Clone, Copy)]
pub struct NodeRef<'ast> {
    ast: &'ast ArchivedAst,
    id: NodeId,
    node: &'ast ArchivedNode,
}

impl<'ast> NodeRef<'ast> {
    /// A cursor at `id`, or `None` if `id` is out of range.
    pub fn at(ast: &'ast ArchivedAst, id: NodeId) -> Option<Self> {
        ast.node(id).map(|node| NodeRef { ast, id, node })
    }

    /// A cursor at the document root, or `None` if the tree is empty.
    pub fn root(ast: &'ast ArchivedAst) -> Option<Self> {
        Self::at(ast, ast.root())
    }

    /// This node's id.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// This node's kind.
    pub fn kind(&self) -> NodeKind {
        self.node.kind()
    }

    /// This node's absolute byte span.
    pub fn span(&self) -> Span {
        self.node.span()
    }

    /// The source text this node covers (empty if the span is somehow out of range).
    pub fn text(&self) -> &'ast str {
        self.ast.text_of(self.node.span()).unwrap_or("")
    }

    /// The parent node, or `None` for the root (whose parent is itself).
    pub fn parent(&self) -> Option<NodeRef<'ast>> {
        if self.id == self.ast.root() {
            None
        } else {
            Self::at(self.ast, self.node.parent())
        }
    }

    /// The first child, if any.
    pub fn first_child(&self) -> Option<NodeRef<'ast>> {
        self.node.first_child().and_then(|c| Self::at(self.ast, c))
    }

    /// The next sibling, if any.
    pub fn next_sibling(&self) -> Option<NodeRef<'ast>> {
        self.node.next_sibling().and_then(|c| Self::at(self.ast, c))
    }

    /// An iterator over the direct children, in order.
    pub fn children(&self) -> Children<'ast> {
        Children {
            next: self.first_child(),
        }
    }
}

/// Two cursors are equal when they point at the same node of the same archive.
impl PartialEq for NodeRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && core::ptr::eq(self.ast, other.ast)
    }
}
impl Eq for NodeRef<'_> {}

impl core::fmt::Debug for NodeRef<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NodeRef")
            .field("id", &self.id)
            .field("kind", &self.kind())
            .finish()
    }
}

/// Iterator over a node's direct children (see [`NodeRef::children`]).
#[derive(Clone)]
pub struct Children<'ast> {
    next: Option<NodeRef<'ast>>,
}

impl<'ast> Iterator for Children<'ast> {
    type Item = NodeRef<'ast>;
    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next?;
        self.next = current.next_sibling();
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use tzlint_ast::{Ast, Node, OptionNodeId};

    /// `# Hi\n\nbody` → Root → [Heading → Text("Hi"), Paragraph → Text("body")].
    fn sample() -> Ast {
        let nodes = vec![
            Node {
                kind: NodeKind::ROOT,
                span: Span::new(0, 10),
                parent: NodeId(0),
                first_child: OptionNodeId::some(NodeId(1)),
                next_sibling: OptionNodeId::NONE,
            },
            Node {
                kind: NodeKind::HEADING,
                span: Span::new(0, 4), // "# Hi"
                parent: NodeId(0),
                first_child: OptionNodeId::some(NodeId(2)),
                next_sibling: OptionNodeId::some(NodeId(3)),
            },
            Node {
                kind: NodeKind::TEXT,
                span: Span::new(2, 4), // "Hi"
                parent: NodeId(1),
                first_child: OptionNodeId::NONE,
                next_sibling: OptionNodeId::NONE,
            },
            Node {
                kind: NodeKind::PARAGRAPH,
                span: Span::new(6, 10), // "body"
                parent: NodeId(0),
                first_child: OptionNodeId::some(NodeId(4)),
                next_sibling: OptionNodeId::NONE,
            },
            Node {
                kind: NodeKind::TEXT,
                span: Span::new(6, 10), // "body"
                parent: NodeId(3),
                first_child: OptionNodeId::NONE,
                next_sibling: OptionNodeId::NONE,
            },
        ];
        Ast {
            nodes,
            text: alloc::string::String::from("# Hi\n\nbody"),
            root: NodeId(0),
        }
    }

    #[test]
    fn navigates_the_archive_in_place() {
        let bytes = tzlint_ast::to_archive(&sample()).unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();

        let root = NodeRef::root(ast).unwrap();
        assert_eq!(root.kind(), NodeKind::ROOT);
        assert_eq!(root.parent(), None); // root's parent is itself → None

        let kids: alloc::vec::Vec<_> = root.children().map(|c| c.kind()).collect();
        assert_eq!(kids, vec![NodeKind::HEADING, NodeKind::PARAGRAPH]);

        let heading = root.first_child().unwrap();
        assert_eq!(heading.text(), "# Hi");
        let para = heading.next_sibling().unwrap();
        assert_eq!(para.kind(), NodeKind::PARAGRAPH);
        assert_eq!(para.text(), "body");
        assert_eq!(para.next_sibling(), None);

        // child → text, and back up to the parent heading.
        let hi = heading.first_child().unwrap();
        assert_eq!(hi.kind(), NodeKind::TEXT);
        assert_eq!(hi.text(), "Hi");
        assert_eq!(hi.parent().unwrap().id(), heading.id());
    }

    #[test]
    fn out_of_range_id_is_none() {
        let bytes = tzlint_ast::to_archive(&sample()).unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();
        assert!(NodeRef::at(ast, NodeId(99)).is_none());
    }

    #[test]
    fn span_equality_and_debug() {
        let bytes = tzlint_ast::to_archive(&sample()).unwrap();
        let ast = tzlint_ast::access(&bytes).unwrap();
        let root = NodeRef::root(ast).unwrap();
        let heading = root.first_child().unwrap();
        let para = heading.next_sibling().unwrap();

        assert_eq!(heading.span(), Span::new(0, 4));

        // Equality is same-node-of-same-archive.
        assert_eq!(heading, NodeRef::at(ast, NodeId(1)).unwrap());
        assert_ne!(heading, para);

        let shown = alloc::format!("{heading:?}");
        assert!(shown.contains("NodeRef"), "{shown}");
        assert!(shown.contains("NodeId(1)"), "{shown}"); // the heading's id
    }
}
