use std::ops::Range;

/// A sentence unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// The text content of the sentence.
    pub text: String,
    /// The absolute byte range of the sentence in the original text (including trailing punctuation).
    pub span: Range<usize>,
}

/// A sentence splitter that handles text with ignored ranges (e.g., inline code).
pub struct SentenceSplitter;

impl SentenceSplitter {
    /// Splits text into sentences, respecting ignored ranges.
    ///
    /// The `ignore_ranges` argument specifies byte ranges that should be treated as opaque blocks.
    /// No sentence boundary will be created inside these ranges.
    ///
    /// Note on newlines:
    /// - `\n\n` (double newline) is treated as a paragraph break and splits sentences.
    /// - Single `\n` is NOT treated as a sentence boundary and is retained as part of the text
    ///   to preserve the exact span and content.
    pub fn split(text: &str, ignore_ranges: &[Range<usize>]) -> Vec<Sentence> {
        let mut sentences = Vec::new();
        let mut start = 0;
        let mut chars = text.char_indices().peekable();

        // Sort ignore ranges just in case
        let mut sorted_ignore = ignore_ranges.to_vec();
        sorted_ignore.sort_by_key(|r| r.start);

        while let Some((idx, c)) = chars.next() {
            // Check if current char is inside an ignored range (binary search on sorted ranges)
            let pos = sorted_ignore.partition_point(|r| r.end <= idx);
            let is_ignored = pos < sorted_ignore.len() && sorted_ignore[pos].start <= idx;

            if is_ignored {
                continue;
            }

            // Simple splitting logic:
            // - Split on `。`, `!`, `?`
            // - Split on double newline `\n\n` (paragraph break)
            // - Handle "." only if followed by space? (For English mixed in) - Keep simple for now.
            // Note: Single punctuation marks are treated as valid sentences to respect user intent.
            let (is_sentence_end, extra_len) = match c {
                '。' | '！' | '？' | '!' | '?' => (true, 0),
                '.' => {
                    // Start simple heuristic: only split on '.' if followed by whitespace or end of text.
                    // This prevents splitting on "3.14", "example.com", "ver.1.0" etc.
                    if let Some((_, next_c)) = chars.peek() {
                        if next_c.is_whitespace() {
                            (true, 0)
                        } else {
                            (false, 0)
                        }
                    } else {
                        (true, 0) // End of text
                    }
                }
                '\n' => {
                    // Check for double newline
                    if let Some((_, next_c)) = chars.peek() {
                        if *next_c == '\n' {
                            // Consume the second newline to include it in the current separator
                            chars.next();
                            (true, 1)
                        } else {
                            (false, 0)
                        }
                    } else {
                        (false, 0)
                    }
                }
                _ => (false, 0),
            };

            if is_sentence_end {
                // Include the punctuation in the sentence
                let end = idx + c.len_utf8() + extra_len;

                let sentence_text = text[start..end].to_string();
                // Avoid empty sentences (e.g., consecutive punctuation)
                if !sentence_text.trim().is_empty() {
                    sentences.push(Sentence {
                        text: sentence_text,
                        span: start..end,
                    });
                }
                start = end;
            }
        }

        // Add remaining text as the last sentence
        if start < text.len() {
            let sentence_text = text[start..].to_string();
            if !sentence_text.trim().is_empty() {
                sentences.push(Sentence {
                    text: sentence_text,
                    span: start..text.len(),
                });
            }
        }

        sentences
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_simple() {
        let text = "こんにちは。世界。";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "こんにちは。");
        assert_eq!(sentences[1].text, "世界。");
    }

    #[test]
    fn test_split_ignore_code() {
        let text = "これは `code.` です。";
        // Mock ignore range for `code.` (from index 10 to 17)
        // "これは " (10 bytes) ` (1 byte) "code." (5 bytes) ` (1 byte) " です。"
        // indices:
        // 0: こ
        // 3: れ
        // 6: は
        // 9:
        // 10: `
        // 11: c
        // 15: . (this is inside code)
        // 16: `

        let ignore_range = 10..17; // `code.`
        let sentences = SentenceSplitter::split(text, &[ignore_range]);

        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "これは `code.` です。");
    }

    #[test]
    fn test_split_empty() {
        let sentences = SentenceSplitter::split("", &[]);
        assert!(sentences.is_empty());
    }

    #[test]
    fn test_split_no_punctuation() {
        let text = "Hello World";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "Hello World");
    }

    #[test]
    fn test_split_consecutive_punctuation() {
        let text = "Hello。。World!?";
        let sentences = SentenceSplitter::split(text, &[]);
        // Expect: "Hello。", "。", "World!", "?"
        // Current logic:
        // 1. "Hello" -> '。' found at end of Hello. sentence="Hello。"
        // 2. Next '。' -> sentence="。"
        // 3. "World" -> '!' found. sentence="World!"
        // 4. Next '?' -> sentence="?"
        assert_eq!(sentences.len(), 4);
        assert_eq!(sentences[0].text, "Hello。");
        assert_eq!(sentences[1].text, "。");
        assert_eq!(sentences[2].text, "World!");
        assert_eq!(sentences[3].text, "?");
    }

    #[test]
    fn test_split_newlines() {
        let text = "Line1.\nLine2.\n\nParagraph2.";
        // Test that:
        // - `.` splits sentences.
        // - `\n\n` (double newline) splits sentences (paragraph break).
        // - Single `\n` does NOT split sentences.

        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0].text, "Line1.");
        assert_eq!(sentences[1].text, "\nLine2."); // newline is part of this sentence start
        assert_eq!(sentences[2].text, "Paragraph2.");
    }

    #[test]
    fn test_split_full_ignore() {
        let text = "A. B.";
        let ignore_range = 0..text.len();
        let sentences = SentenceSplitter::split(text, &[ignore_range]);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "A. B.");
    }

    #[test]
    fn test_split_double_newline_behavior() {
        let text = "A\n\nB";
        let sentences = SentenceSplitter::split(text, &[]);
        // Current behavior (before fix): "A\n", "\nB" (or "A\n" and "B" if \n is trimmed?)
        // Desired behavior: "A\n\n", "B"

        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "A\n\n");
        assert_eq!(sentences[1].text, "B");
    }

    #[test]
    fn test_split_english_mixed() {
        let text = "This is ver.1.0. Please visit example.com.";
        let sentences = SentenceSplitter::split(text, &[]);
        // Should split at first "ver.1.0. " (because of trailing space) and last "."
        // "ver.1.0" -> '.' followed by '0' -> no split
        // "1.0." -> '.' followed by space -> split
        // "example.com." -> '.' followed by EOF -> split

        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "This is ver.1.0.");
        assert_eq!(sentences[1].text, " Please visit example.com.");
    }

    #[test]
    fn test_split_english_abbreviations() {
        let text = "e.g. example vs. sample";
        let sentences = SentenceSplitter::split(text, &[]);
        // "e.g. " -> split
        // "example vs. " -> split
        // "sample" -> remainder

        // Current heuristic splits on "e.g. " twice?
        // "e." -> followed by 'g' -> no split
        // "g. " -> followed by space -> split "e.g. "
        // "vs. " -> followed by space -> split "example vs. "

        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0].text, "e.g.");
        assert_eq!(sentences[1].text, " example vs.");
        assert_eq!(sentences[2].text, " sample");
    }
}
