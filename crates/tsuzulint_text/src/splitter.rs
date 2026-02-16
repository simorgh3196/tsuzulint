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
            // - Split on `。` (Kuten) always.
            // - Split on `!`, `?` (and variants) ONLY if followed by whitespace or EOF.
            // - Split on double newline `\n\n` (paragraph break).
            // - Handle "." only if followed by space or EOF.
            let (is_sentence_end, extra_len) = match c {
                '。' => (true, 0),
                '！' | '？' | '!' | '?' | '.' => {
                    // Check if followed by whitespace or end of text.
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
        // "Hello。。" -> "Hello。" and "。"
        // "World!?" -> No split (no space).
        // "World!?" -> followed by EOF -> Split.
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0].text, "Hello。");
        assert_eq!(sentences[1].text, "。");
        assert_eq!(sentences[2].text, "World!?");
    }

    #[test]
    fn test_split_no_space_exclamation() {
        let text = "すごい！！本当に！？";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "すごい！！本当に！？");
    }

    #[test]
    fn test_split_with_space_exclamation() {
        let text = "すごい！！ 本当に！？";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "すごい！！");
        assert_eq!(sentences[1].text, " 本当に！？");
    }

    #[test]
    fn test_split_newlines() {
        let text = "Line1.\nLine2.\n\nParagraph2.";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0].text, "Line1.");
        assert_eq!(sentences[1].text, "\nLine2.");
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
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "A\n\n");
        assert_eq!(sentences[1].text, "B");
    }

    #[test]
    fn test_split_english_mixed() {
        let text = "This is ver.1.0. Please visit example.com.";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "This is ver.1.0.");
        assert_eq!(sentences[1].text, " Please visit example.com.");
    }

    #[test]
    fn test_split_english_abbreviations() {
        let text = "e.g. example vs. sample";
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0].text, "e.g.");
        assert_eq!(sentences[1].text, " example vs.");
        assert_eq!(sentences[2].text, " sample");
    }
}
