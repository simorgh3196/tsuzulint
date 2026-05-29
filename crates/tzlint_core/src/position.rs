//! Byte-offset → 1-based `(line, column)` mapping for diagnostic output.
//!
//! Diagnostics carry absolute byte [`Span`](tzlint_ast::Span)s into the source; this
//! converts an offset to a human position once, at output time.

/// Precomputed line-start offsets for a source text, mapping an absolute **byte** offset
/// to a 1-based `(line, column)` position.
///
/// Column is counted in **Unicode scalar values** (chars) from the start of the line,
/// matching typical CLI output; a UTF-16 column (for LSP) can be layered on later. Build
/// the index with the same text whose offsets you will map.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset at which each line begins. Always starts with `0`.
    line_starts: Vec<u32>,
}

impl LineIndex {
    /// Build the index for `text`.
    pub fn new(text: &str) -> Self {
        let mut line_starts = Vec::with_capacity(16);
        line_starts.push(0);
        for (i, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self { line_starts }
    }

    /// Number of lines (always ≥ 1).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Map an absolute byte `offset` into `text` to a 1-based `(line, column)`.
    ///
    /// `text` must be the same source passed to [`LineIndex::new`]. An out-of-range or
    /// non-char-boundary offset is clamped, never panics.
    pub fn position(&self, text: &str, offset: u32) -> (u32, u32) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line_idx).copied().unwrap_or(0) as usize;
        let end = (offset as usize).min(text.len());
        let start = line_start.min(end);
        let column = text
            .get(start..end)
            .map_or(0, |slice| slice.chars().count());
        (line_idx as u32 + 1, column as u32 + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let text = "hello";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.position(text, 0), (1, 1));
        assert_eq!(idx.position(text, 3), (1, 4));
        assert_eq!(idx.position(text, 5), (1, 6)); // end of text
    }

    #[test]
    fn multi_line() {
        let text = "ab\ncde\nf";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_count(), 3);
        assert_eq!(idx.position(text, 0), (1, 1)); // 'a'
        assert_eq!(idx.position(text, 2), (1, 3)); // '\n' is column 3 of line 1
        assert_eq!(idx.position(text, 3), (2, 1)); // 'c' starts line 2
        assert_eq!(idx.position(text, 6), (2, 4)); // '\n' at end of line 2
        assert_eq!(idx.position(text, 7), (3, 1)); // 'f' starts line 3
    }

    #[test]
    fn columns_count_chars_not_bytes() {
        // CJK: each char is 3 UTF-8 bytes but one column.
        let text = "あいう\nx"; // 9 bytes + '\n' + 'x'
        let idx = LineIndex::new(text);
        assert_eq!(idx.position(text, 0), (1, 1)); // あ
        assert_eq!(idx.position(text, 3), (1, 2)); // い
        assert_eq!(idx.position(text, 6), (1, 3)); // う
        assert_eq!(idx.position(text, 9), (1, 4)); // '\n'
        assert_eq!(idx.position(text, 10), (2, 1)); // x
    }

    #[test]
    fn empty_text() {
        let text = "";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.position(text, 0), (1, 1));
    }

    #[test]
    fn out_of_range_offset_is_clamped() {
        let text = "ab\ncd";
        let idx = LineIndex::new(text);
        // Past the end: clamped to the end of the last line, no panic.
        assert_eq!(idx.position(text, 999), (2, 3));
    }

    #[test]
    fn trailing_newline_creates_empty_final_line() {
        let text = "a\n";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_count(), 2);
        assert_eq!(idx.position(text, 2), (2, 1));
    }
}
