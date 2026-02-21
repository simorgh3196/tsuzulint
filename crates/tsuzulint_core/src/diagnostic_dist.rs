//! Diagnostic distribution to blocks for incremental caching.

use std::collections::HashSet;
use tsuzulint_cache::entry::BlockCacheEntry;
use tsuzulint_plugin::Diagnostic;

/// Distributes diagnostics to blocks efficiently using a sorted cursor approach.
///
/// This optimization reduces complexity from O(Blocks * Diagnostics) to O(Blocks + Diagnostics)
/// (plus sorting cost O(B log B + D log D)), which is significant for large files with many blocks.
///
/// # Preconditions
///
/// - `diagnostics` must be sorted by start offset. This is a contract with the caller.
pub fn distribute_diagnostics(
    mut blocks: Vec<BlockCacheEntry>,
    diagnostics: &[Diagnostic],
    global_keys: &HashSet<&Diagnostic>,
) -> Vec<BlockCacheEntry> {
    debug_assert!(
        diagnostics
            .windows(2)
            .all(|w| w[0].span.start <= w[1].span.start),
        "distribute_diagnostics: diagnostics must be sorted by span.start"
    );

    blocks.sort_by_key(|b| b.span.start);

    let local_diagnostics: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| !global_keys.contains(d))
        .collect();

    let mut diag_idx = 0;

    blocks
        .into_iter()
        .map(|mut block| {
            while diag_idx < local_diagnostics.len()
                && local_diagnostics[diag_idx].span.start < block.span.start
            {
                diag_idx += 1;
            }

            let mut block_diags = Vec::new();
            let mut temp_idx = diag_idx;

            while temp_idx < local_diagnostics.len() {
                let diag = local_diagnostics[temp_idx];

                if diag.span.start >= block.span.end {
                    break;
                }

                if diag.span.end <= block.span.end {
                    block_diags.push(diag.clone());
                }

                temp_idx += 1;
            }

            block.diagnostics = block_diags;
            block
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_ast::Span;
    use tsuzulint_plugin::Severity;

    fn make_diag(rule_id: &str, start: u32, end: u32) -> Diagnostic {
        Diagnostic {
            rule_id: rule_id.to_string(),
            message: format!("msg_{}", rule_id),
            span: Span::new(start, end),
            severity: Severity::Error,
            fix: None,
            loc: None,
        }
    }

    fn make_block(hash_byte: u8, start: u32, end: u32) -> BlockCacheEntry {
        BlockCacheEntry {
            hash: [hash_byte; 32],
            span: Span::new(start, end),
            diagnostics: vec![],
        }
    }

    #[test]
    fn test_distribute_diagnostics_basic() {
        let global_keys = HashSet::new();

        let block1 = make_block(1, 10, 20);
        let block2 = make_block(2, 30, 40);
        let blocks = vec![block1, block2];

        let diag1 = make_diag("rule1", 12, 15);
        let diag2 = make_diag("rule2", 32, 35);
        let diag_outside = make_diag("rule3", 0, 5);
        let diag_overlap = make_diag("rule4", 15, 25);

        let mut diagnostics = vec![diag2.clone(), diag1.clone(), diag_outside, diag_overlap];
        diagnostics.sort_unstable();

        let result = distribute_diagnostics(blocks.clone(), &diagnostics, &global_keys);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].diagnostics.len(), 1);
        assert_eq!(result[0].diagnostics[0].rule_id, "rule1");
        assert_eq!(result[1].diagnostics.len(), 1);
        assert_eq!(result[1].diagnostics[0].rule_id, "rule2");
    }

    #[test]
    fn test_distribute_diagnostics_filter_global() {
        let block1 = make_block(1, 10, 20);
        let block2 = make_block(2, 30, 40);
        let blocks = vec![block1, block2];

        let diag1 = make_diag("rule1", 12, 15);
        let diag2 = make_diag("rule2", 32, 35);
        let mut diagnostics = vec![diag1.clone(), diag2.clone()];
        diagnostics.sort_unstable();

        let mut global_keys = HashSet::new();
        global_keys.insert(&diagnostics[0]);

        let result = distribute_diagnostics(blocks.clone(), &diagnostics, &global_keys);

        assert!(result[0].diagnostics.is_empty());
        assert_eq!(result[1].diagnostics.len(), 1);
        assert_eq!(result[1].diagnostics[0].rule_id, "rule2");
    }

    #[test]
    fn test_distribute_diagnostics_boundary() {
        let block = make_block(3, 10, 20);
        let diag_at_end = make_diag("boundary", 20, 25);
        let diag_zero_at_end = make_diag("zero", 20, 20);

        let mut diagnostics = vec![diag_at_end, diag_zero_at_end];
        diagnostics.sort_by_key(|d| d.span.start);

        let result = distribute_diagnostics(vec![block], &diagnostics, &HashSet::new());

        assert!(result[0].diagnostics.is_empty());
    }
}
