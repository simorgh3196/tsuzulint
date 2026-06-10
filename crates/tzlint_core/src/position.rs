//! Byte-offset → 1-based `(line, column)` mapping for diagnostic output.
//!
//! Diagnostics carry absolute byte [`Span`](tzlint_ast::Span)s into the source; this
//! converts an offset to a human position once, at output time.

/// Precomputed line-start offsets for a source text, mapping an absolute **byte** offset
/// to a 1-based `(line, column)` position.
///
/// Column is counted in **Unicode scalar values** (chars) from the start of the line,
/// matching typical CLI output; [`utf16_column`](LineIndex::utf16_column) layers a UTF-16
/// code-unit column on top (for editors/LSP that address text in UTF-16). Build the index with
/// the same text whose offsets you will map.
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
        let bytes = text.as_bytes();
        for (i, &byte) in bytes.iter().enumerate() {
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

    /// Clamp `offset` into range and down to a char boundary, then locate its line.
    ///
    /// Returns the 0-based line index plus the `start..end` byte range of the line prefix up to
    /// the (clamped) offset — the slice whose length, counted in some unit, is the column. An
    /// offset that lands inside a multibyte char floors to that char's start; since a multibyte
    /// char never straddles a newline, this stays on the same line. The returned range is always
    /// a valid `text` slice, so callers can `text.get(start..end)` without panicking.
    fn locate(&self, text: &str, offset: u32) -> (usize, usize, usize) {
        let mut end = (offset as usize).min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let end_u32 = end as u32;
        let line_idx = self
            .line_starts
            .partition_point(|&x| x <= end_u32)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line_idx).copied().unwrap_or(0) as usize;
        let start = line_start.min(end);
        (line_idx, start, end)
    }

    /// Map an absolute byte `offset` into `text` to a 1-based `(line, column)`.
    ///
    /// Column is counted in Unicode scalar values. `text` must be the same source passed to
    /// [`LineIndex::new`]. An out-of-range or non-char-boundary offset is clamped, never panics.
    pub fn position(&self, text: &str, offset: u32) -> (u32, u32) {
        let (line_idx, start, end) = self.locate(text, offset);
        let column = text.get(start..end).map_or(0, |slice| {
            let mut count = 0;
            for &b in slice.as_bytes() {
                // In UTF-8, any byte that is not a continuation byte (which is 10xxxxxx, i.e. 0x80..=0xBF)
                // is the start of a character. We can identify these efficiently by casting to i8.
                if (b as i8) >= -0x40 {
                    count += 1;
                }
            }
            count
        });
        (line_idx as u32 + 1, column as u32 + 1)
    }

    /// The 1-based **UTF-16 code-unit** column for an absolute byte `offset`.
    ///
    /// Counts UTF-16 code units from the start of the line, so a BMP char counts as 1 and an
    /// astral-plane char (≥ U+10000, encoded as a surrogate pair) counts as 2 — the column
    /// editors and the LSP use, which address text in UTF-16. The line is the same one
    /// [`position`](Self::position) reports; clamping is identical. Add this alongside the scalar
    /// column rather than replacing it.
    pub fn utf16_column(&self, text: &str, offset: u32) -> u32 {
        let (_, start, end) = self.locate(text, offset);
        let units = text
            .get(start..end)
            .map_or(0, |slice| slice.chars().map(char::len_utf16).sum::<usize>());
        units as u32 + 1
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

    #[test]
    fn non_char_boundary_offset_clamps_to_its_char() {
        let text = "あい"; // 2 chars, 6 bytes; boundaries at 0, 3, 6
        let idx = LineIndex::new(text);
        assert_eq!(idx.position(text, 0), (1, 1)); // start of あ
        assert_eq!(idx.position(text, 1), (1, 1)); // inside あ → floors to 0 → column 1
        assert_eq!(idx.position(text, 3), (1, 2)); // start of い
        assert_eq!(idx.position(text, 4), (1, 2)); // inside い → floors to 3 → column 2
    }

    #[test]
    fn utf16_column_matches_scalar_for_bmp() {
        // Every BMP char (including CJK) is a single UTF-16 code unit, so the UTF-16 column
        // equals the scalar column there.
        let text = "あいう\nx"; // 9 bytes + '\n' + 'x'
        let idx = LineIndex::new(text);
        assert_eq!(idx.utf16_column(text, 0), 1); // あ
        assert_eq!(idx.utf16_column(text, 3), 2); // い
        assert_eq!(idx.utf16_column(text, 6), 3); // う
        assert_eq!(idx.utf16_column(text, 9), 4); // '\n'
        assert_eq!(idx.utf16_column(text, 10), 1); // x on line 2 — UTF-16 column resets per line
    }

    #[test]
    fn utf16_column_counts_astral_chars_as_two_units() {
        // "😀" is U+1F600 (astral plane): 4 UTF-8 bytes but 2 UTF-16 code units (a surrogate pair).
        let text = "😀x"; // 😀 occupies bytes 0..4; 'x' starts at byte 4
        let idx = LineIndex::new(text);
        assert_eq!(idx.utf16_column(text, 0), 1); // start of 😀
        // Before 'x' there is one scalar value but two UTF-16 units.
        assert_eq!(idx.position(text, 4), (1, 2)); // scalar column
        assert_eq!(idx.utf16_column(text, 4), 3); // UTF-16 column
    }

    #[test]
    fn utf16_column_clamps_like_position() {
        let text = "😀"; // boundaries at 0 and 4
        let idx = LineIndex::new(text);
        assert_eq!(idx.utf16_column(text, 2), 1); // inside the astral char → floors to its start
        assert_eq!(idx.utf16_column(text, 999), 3); // past the end → clamps to end (2 units + 1)
    }
}
