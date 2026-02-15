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
    pub fn split(text: &str, ignore_ranges: &[Range<usize>]) -> Vec<Sentence> {
        let mut sentences = Vec::new();
        let mut start = 0;
        let mut chars = text.char_indices().peekable();

        // Sort ignore ranges just in case
        let mut sorted_ignore = ignore_ranges.to_vec();
        sorted_ignore.sort_by_key(|r| r.start);

        while let Some((idx, c)) = chars.next() {
            // Check if current char is inside an ignored range
            let is_ignored = sorted_ignore
                .iter()
                .any(|range| idx >= range.start && idx < range.end);

            if is_ignored {
                continue;
            }

            // Simple splitting logic:
            // - Split on `。`, `!`, `?`
            // - Split on double newline `\n\n` (paragraph break)
            // - Handle "." only if followed by space? (For English mixed in) - Keep simple for now.
            let is_sentence_end = match c {
                '。' | '！' | '？' | '!' | '?' | '.' => true,
                '\n' => {
                    // Check for double newline
                    if let Some((_, next_c)) = chars.peek() {
                        *next_c == '\n'
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if is_sentence_end {
                // Include the punctuation in the sentence
                let end = idx + c.len_utf8();

                // If it was a double newline, we might want to exclude the second newline from this sentence
                // But for simplicity, let's just use the current position.

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
        // Mock ignore range for `code.` (from index 4 to 11)
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
        // \n is NOT a split char, but \n\n IS.
        // But '.' IS a split char.
        // "Line1." -> split at '.' -> "Line1."
        // "\nLine2." -> split at '.' -> "\nLine2."
        // "\n\nParagraph2." -> split at '.' -> "\n\nParagraph2."
        // Wait, \n\n logic:
        // At \n, lookahead \n. If true, it is sentence end.
        // Let's trace:
        // text: "A\n\nB"
        // 'A' -> next
        // '\n' -> peek '\n' -> is_sentence_end=true. end=idx+1 (start of second \n).
        // sentence="A\n". start points to second \n.
        // next loop: char is '\n'. peek 'B'. is_sentence_end=false.
        // 'B' -> next.
        // End of loop. Remaining: "\nB".
        // So "A\n\nB" -> "A\n", "\nB" ?
        // This splits paragraph.

        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0].text, "Line1.");
        assert_eq!(sentences[1].text, "\nLine2."); // newline is part of this sentence start
        assert_eq!(sentences[2].text, "\nParagraph2.");
    }

    #[test]
    fn test_split_full_ignore() {
        let text = "A. B.";
        let ignore_range = 0..text.len();
        let sentences = SentenceSplitter::split(text, &[ignore_range]);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "A. B.");
    }
}
