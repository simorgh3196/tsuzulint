//! A small RFC 4180-style delimited scanner that yields, per record, the absolute byte
//! **content** span of each cell. Quoted-cell spans are the bytes inside the outer quotes;
//! embedded newlines and doubled quotes (`""`) stay in the (contiguous) span as raw bytes.
//! See `docs/design/input-format-processors.md` §6.

use tzlint_ast::Span;

/// Scan `source` into records of cell **content** spans (absolute byte offsets). Records split
/// on unquoted CR / LF / CRLF; fields split on the unquoted `delimiter`. A leading UTF-8 BOM is
/// skipped. Never panics: an unterminated quote takes content to end-of-input.
pub(crate) fn scan_records(source: &str, delimiter: u8) -> Vec<Vec<Span>> {
    let b = source.as_bytes();
    let n = b.len();
    let mut i = if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };

    let mut records: Vec<Vec<Span>> = Vec::new();
    if i >= n {
        return records; // empty (or BOM-only) input → no records
    }
    let mut record: Vec<Span> = Vec::new();

    loop {
        // --- parse one field beginning at `i` ---
        let (content_start, content_end, after_field);
        if b.get(i) == Some(&b'"') {
            // Quoted field: content runs from just after the opening quote to the closing quote.
            let start = i + 1;
            let mut j = start;
            loop {
                match b.get(j) {
                    None => break, // unterminated → content to EOF
                    Some(&b'"') => {
                        if b.get(j + 1) == Some(&b'"') {
                            j += 2; // escaped quote, stays in content
                            continue;
                        }
                        break; // closing quote
                    }
                    Some(_) => j += 1,
                }
            }
            content_start = start;
            content_end = j.min(n);
            after_field = if j < n { j + 1 } else { n }; // skip the closing quote
        } else {
            // Unquoted field: runs to the next delimiter or newline.
            let mut j = i;
            while j < n && b[j] != delimiter && b[j] != b'\n' && b[j] != b'\r' {
                j += 1;
            }
            content_start = i;
            content_end = j;
            after_field = j;
        }
        // `as u32` is safe: byte offsets index `source`, and `io::MAX_FILE` (16 MiB) is far below
        // `u32::MAX`, so an offset always fits.
        record.push(Span::new(content_start as u32, content_end as u32));

        // --- advance to the separator (delimiter / newline / EOF) ---
        let mut k = after_field;
        while k < n && b[k] != delimiter && b[k] != b'\n' && b[k] != b'\r' {
            k += 1; // tolerate any trailing bytes after a closing quote
        }
        if k >= n {
            records.push(core::mem::take(&mut record));
            break;
        }
        let sep = b[k];
        if sep == delimiter {
            i = k + 1; // another field in the same record
        } else {
            // sep is CR or LF — end of record.
            records.push(core::mem::take(&mut record));
            i = if sep == b'\r' && b.get(k + 1) == Some(&b'\n') {
                k + 2
            } else {
                k + 1
            };
            if i >= n {
                break; // trailing newline does not create a spurious empty record
            }
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: scan and project each cell span to the &str it covers.
    fn cells(source: &str, delim: u8) -> Vec<Vec<&str>> {
        scan_records(source, delim)
            .into_iter()
            .map(|rec| {
                rec.into_iter()
                    .map(|s| {
                        source
                            .get(s.start as usize..s.end as usize)
                            .unwrap_or("<bad>")
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn simple_rows_and_fields() {
        assert_eq!(
            cells("a,b,c\n1,2,3\n", b','),
            vec![vec!["a", "b", "c"], vec!["1", "2", "3"]],
        );
    }

    #[test]
    fn final_line_without_newline() {
        assert_eq!(
            cells("a,b\nc,d", b','),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
    }

    #[test]
    fn quoted_field_content_excludes_quotes() {
        // The middle field is quoted and contains the delimiter; its content span is `x,y`.
        assert_eq!(cells("a,\"x,y\",b\n", b','), vec![vec!["a", "x,y", "b"]]);
    }

    #[test]
    fn quoted_field_with_embedded_newline_stays_in_one_cell() {
        let rows = cells("\"line1\nline2\",b\n", b',');
        assert_eq!(rows, vec![vec!["line1\nline2", "b"]]);
    }

    #[test]
    fn escaped_quotes_remain_raw_in_span() {
        // `"He said ""hi"""` → content span is `He said ""hi""` (raw doubled quotes; v1).
        assert_eq!(
            cells("\"He said \"\"hi\"\"\"\n", b','),
            vec![vec!["He said \"\"hi\"\""]]
        );
    }

    #[test]
    fn crlf_line_endings() {
        assert_eq!(
            cells("a,b\r\nc,d\r\n", b','),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
    }

    #[test]
    fn tab_delimiter() {
        assert_eq!(
            cells("a\tb\n1\t2\n", b'\t'),
            vec![vec!["a", "b"], vec!["1", "2"]]
        );
    }

    #[test]
    fn leading_bom_is_skipped() {
        assert_eq!(cells("\u{feff}a,b\n", b','), vec![vec!["a", "b"]]);
    }

    #[test]
    fn ragged_rows_keep_their_field_counts() {
        assert_eq!(
            cells("a,b,c\nx,y\n", b','),
            vec![vec!["a", "b", "c"], vec!["x", "y"]]
        );
    }

    #[test]
    fn empty_input_yields_no_records() {
        assert!(scan_records("", b',').is_empty());
        assert!(scan_records("\u{feff}", b',').is_empty());
    }

    #[test]
    fn never_panics_on_unterminated_quote() {
        // Best-effort: an unterminated quote takes content to EOF, no panic — and the recovered
        // content (inside the opening quote) runs all the way to end-of-input.
        let records = scan_records("\"abc", b',');
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].len(), 1);
        let span = records[0][0];
        assert_eq!(span, Span::new(1, 4)); // content starts just after the opening quote, to EOF
        assert_eq!(cells("\"abc", b','), vec![vec!["abc"]]);
    }

    #[test]
    fn cell_spans_are_exact_byte_offsets() {
        // The scanner's contract is its spans, so assert raw `Span` values, not just text.
        // `a,"x,y",b\n` → field 0 = bytes 0..1, field 1 (quoted) = inside the quotes 3..6,
        // field 2 = bytes 8..9.
        let records = scan_records("a,\"x,y\",b\n", b',');
        assert_eq!(records.len(), 1);
        let row = &records[0];
        assert_eq!(row[0], Span::new(0, 1));
        assert_eq!(row[1], Span::new(3, 6));
        assert_eq!(row[2], Span::new(8, 9));
    }

    #[test]
    fn trailing_delimiter_yields_empty_last_field() {
        // A trailing delimiter produces an empty final field, with or without a final newline.
        assert_eq!(cells("a,b,\n", b','), vec![vec!["a", "b", ""]]);
        assert_eq!(cells("a,b,", b','), vec![vec!["a", "b", ""]]);
    }

    #[test]
    fn lone_cr_separates_records() {
        // A bare CR (no following LF) ends a record, like classic Mac line endings.
        assert_eq!(
            cells("a,b\rc,d\r", b','),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
    }

    #[test]
    fn trailing_bytes_after_closing_quote_are_skipped() {
        // Junk between a closing quote and the next delimiter/newline is tolerated and dropped:
        // the cell content is the quoted span only, and the junk is not part of any cell.
        assert_eq!(cells("\"x\"junk,y\n", b','), vec![vec!["x", "y"]]);
        // …and at end-of-record too (junk after the closing quote before EOF).
        assert_eq!(cells("a,\"b\"tail", b','), vec![vec!["a", "b"]]);
    }
}
