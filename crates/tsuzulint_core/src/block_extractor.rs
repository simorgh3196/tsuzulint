//! Block extraction for incremental caching.

use tracing::warn;
use tsuzulint_ast::{NodeType, TxtNode};
use tsuzulint_cache::{CacheManager, entry::BlockCacheEntry};

pub fn extract_blocks(ast: &TxtNode, content: &str) -> Vec<BlockCacheEntry> {
    let capacity = if ast.node_type == NodeType::Document {
        ast.children.len()
    } else {
        0
    };
    let mut blocks = Vec::with_capacity(capacity);

    visit_blocks(ast, &mut |node| {
        let start = node.span.start as usize;
        let end = node.span.end as usize;
        let content_bytes = content.as_bytes();

        if start <= content_bytes.len() && end <= content_bytes.len() && start <= end {
            let hash = if let Some(slice) = content.get(start..end) {
                CacheManager::hash_content(slice)
            } else {
                let bytes = &content_bytes[start..end];
                let block_content = String::from_utf8_lossy(bytes);
                CacheManager::hash_content(&block_content)
            };

            blocks.push(BlockCacheEntry {
                hash,
                span: node.span,
                diagnostics: Vec::new(),
            });
        } else {
            warn!(
                "Block span {:?} out of bounds for content length {}",
                node.span,
                content.len()
            );
        }
    });

    blocks
}

pub fn visit_blocks<F>(node: &TxtNode, f: &mut F)
where
    F: FnMut(&TxtNode),
{
    if node.node_type == NodeType::Document {
        for child in node.children.iter() {
            f(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::{AstArena, NodeType, Span, TxtNode};

    #[test]
    fn test_extract_blocks_empty_document() {
        let _arena = AstArena::new();
        let doc = TxtNode::new_parent(NodeType::Document, Span::new(0, 0), &[]);

        let blocks = extract_blocks(&doc, "");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_extract_blocks_with_content() {
        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 5), "Hello"));
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 5),
            arena.alloc_slice_copy(&[*text]),
        ));
        let doc = TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 5),
            arena.alloc_slice_copy(&[*para]),
        );

        let blocks = extract_blocks(&doc, "Hello");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].span.start, 0);
        assert_eq!(blocks[0].span.end, 5);
    }

    #[test]
    fn test_extract_blocks_handles_out_of_bounds_gracefully() {
        let arena = AstArena::new();
        let text = arena.alloc(TxtNode::new_text(NodeType::Str, Span::new(0, 100), ""));
        let para = arena.alloc(TxtNode::new_parent(
            NodeType::Paragraph,
            Span::new(0, 100),
            arena.alloc_slice_copy(&[*text]),
        ));
        let doc = TxtNode::new_parent(
            NodeType::Document,
            Span::new(0, 100),
            arena.alloc_slice_copy(&[*para]),
        );

        let blocks = extract_blocks(&doc, "short");
        assert!(blocks.is_empty());
    }
}
