//! Ignore range extraction for code blocks.

use std::ops::ControlFlow;
use tsuzulint_ast::TxtNode;
use tsuzulint_ast::visitor::{VisitResult, Visitor, walk_node};

pub fn extract_ignore_ranges(ast: &TxtNode) -> Vec<std::ops::Range<usize>> {
    struct CodeRangeCollector {
        ranges: Vec<std::ops::Range<usize>>,
    }

    impl<'a> Visitor<'a> for CodeRangeCollector {
        fn visit_code_block(&mut self, node: &TxtNode<'a>) -> VisitResult {
            self.ranges
                .push(node.span.start as usize..node.span.end as usize);
            ControlFlow::Continue(())
        }

        fn visit_code(&mut self, node: &TxtNode<'a>) -> VisitResult {
            self.ranges
                .push(node.span.start as usize..node.span.end as usize);
            ControlFlow::Continue(())
        }
    }

    let mut collector = CodeRangeCollector { ranges: Vec::new() };
    let _ = walk_node(&mut collector, ast);
    collector.ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::AstArena;
    use tsuzulint_parser::{MarkdownParser, Parser};

    #[test]
    fn test_extract_ignore_ranges() {
        let content = "Text.\n```rust\ncode.\n```\nInline `code.` here.";
        let arena = AstArena::new();
        let parser = MarkdownParser::new();
        let ast = parser.parse(&arena, content).unwrap();

        let ranges = extract_ignore_ranges(&ast);

        assert_eq!(ranges.len(), 2, "Expected 2 ignored ranges");

        let r1 = &ranges[0];
        let r2 = &ranges[1];

        let (block, inline) = if r1.start < r2.start {
            (r1, r2)
        } else {
            (r2, r1)
        };

        let block_text = &content[block.clone()];
        assert!(
            block_text.starts_with("```"),
            "First range should be code block"
        );

        let inline_text = &content[inline.clone()];
        assert_eq!(inline_text, "`code.`", "Second range should be inline code");
    }
}
