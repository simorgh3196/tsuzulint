use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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
    /// Splits text into sentences using UAX #29 rules with Japanese-specific heuristics,
    /// while respecting ignored ranges.
    ///
    /// The `ignore_ranges` argument specifies byte ranges that should be treated as opaque blocks.
    /// No sentence boundary will be created inside these ranges.
    pub fn split(text: &str, ignore_ranges: &[Range<usize>]) -> Vec<Sentence> {
        let mut sentences = Vec::new();
        let mut start = 0;

        // Sort ignore ranges
        let mut sorted_ignore = ignore_ranges.to_vec();
        sorted_ignore.sort_by_key(|r| r.start);

        // Get UAX #29 sentence boundaries
        // `unicode_sentence_indices` returns the start index of each sentence.
        // We are interested in the END of each sentence to check for splitting.
        // The iterator gives (start_offset, sentence_str).
        // Standard UAX #29 splits: "Hello! World" -> "Hello! ", "World"
        // "Hello!World" -> "Hello!World" (no split)
        // "すごい！！本当に！？" -> "すごい！！", "本当に！？" (Standard splits here!)

        // `unicode_sentences` returns an iterator of &str (slices).
        // Standard `unicode-segmentation` does not provide `unicode_sentence_indices` apparently,
        // `unicode_sentences` returns slices of the original string.
        // We calculate the byte offset by pointer arithmetic safely.

        let mut segment_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        let mut last_end = 0;

        for s in text.unicode_sentences() {
            // Safe pointer arithmetic to get the byte offset of the slice within `text`.
            // `s` is guaranteed to be a slice of `text` when returned by `unicode-segmentation`.
            let start = s.as_ptr() as usize - text.as_ptr() as usize;

            // If gap, extend previous range (and if we have a previous range)
            if start > last_end
                && let Some(last_range) = segment_ranges.last_mut()
            {
                // Extend previous end to cover the gap
                // The gap becomes part of the *previous* sentence.
                last_range.end = start;
            }

            segment_ranges.push(start..start + s.len());
            last_end = start + s.len();
        }

        // Handle trailing gap (e.g. final newline)
        if last_end < text.len()
            && let Some(last_range) = segment_ranges.last_mut()
        {
            last_range.end = text.len();
        }

        // Iterate through segments to decide whether to split or merge
        for (i, range) in segment_ranges.iter().enumerate() {
            let seg_end = range.end;

            // If this is the last segment, we just finish the current sentence.
            if i == segment_ranges.len() - 1 {
                let sentence_text = text[start..seg_end].to_string();
                if !sentence_text.trim().is_empty() {
                    sentences.push(Sentence {
                        text: sentence_text,
                        span: start..seg_end,
                    });
                }
                break;
            }

            // Check if this split point (seg_end) is valid according to our heuristics.
            if Self::should_split(text, seg_end, &sorted_ignore) {
                let sentence_text = text[start..seg_end].to_string();
                if !sentence_text.trim().is_empty() {
                    sentences.push(Sentence {
                        text: sentence_text,
                        span: start..seg_end,
                    });
                }
                start = seg_end;
            } else {
                // Suppress split: Continue to next segment, extending the current sentence.
                continue;
            }
        }

        sentences
    }

    /// Determines if a split should occur at the given index `idx`.
    fn should_split(text: &str, idx: usize, ignore_ranges: &[Range<usize>]) -> bool {
        // 1. Check if inside ignored range
        // Find if idx is strictly inside an ignore range (start < idx < end).
        // Boundary at start or end of ignore range is usually fine, but
        // if text[start..end] is "code.", UAX splits after ".".
        // If "code." is ignored, we should NOT split strictly inside it.
        // Let's use `partition_point`.
        let pos = ignore_ranges.partition_point(|r| r.end <= idx);
        if pos < ignore_ranges.len() {
            let r = &ignore_ranges[pos];
            // If idx is within (r.start, r.end], it's inside or at the end of an ignored block.
            // If idx is at r.end, it means the ignored block ended. We usually allow split there
            // IF the ignored block itself is a sentence.
            // For safety: if strictly inside, return false.
            if r.start < idx && idx < r.end {
                return false;
            }
        }

        // 2. Character context analysis
        // Look at characters preceding the split point.
        let prev_char = text[..idx].chars().last();

        // Look at the character immediately following the split point.
        let next_char = text[idx..].chars().next();

        match prev_char {
            Some('。') | Some('！') | Some('？') | Some('!') | Some('?') => {
                // 3. Heuristic: `。` always splits. (Assuming UAX found a boundary here)
                if prev_char == Some('。') {
                    return true;
                }

                // 4. Heuristic: `!` `?` only split if followed by space or newline (or EOF).
                // UAX #29 usually DOES split "すごい！！本当に！？" (No space).
                // We want to suppress this if there is NO space.
                if let Some(nc) = next_char {
                    if nc.is_whitespace() {
                        // Check for single newline suppression
                        if nc == '\n' {
                            // Check for double newline
                            // If `\n` is followed by another `\n`, it's a paragraph break -> Keep split.
                            // If `\n` is followed by text -> Suppress split (treat as continuation).
                            // If `\n` is followed by text -> Suppress split (treat as continuation).
                            let after_newline = text[idx + 1..].chars().next();
                            return after_newline == Some('\n');
                        }
                        return true; // Space, etc.
                    } else {
                        // Not whitespace (e.g. "本" in "！！本当に") -> Suppress split.
                        return false;
                    }
                } else {
                    return true; // EOF -> Split.
                }
            }
            // 5. Heuristic: Single newline check for other cases
            // If UAX split on a newline (e.g. after a period + newline), check double newline.
            Some('\n') => {
                // If the PREVIOUS char was `\n`, it's a double newline (paragraph).
                // We need to look further back.
                if text[..idx].ends_with("\n\n") {
                    return true;
                }

                if next_char == Some('\n') {
                    return true;
                }

                // If the split happened AT a newline char (meaning the segment ENDED with \n),
                // and it's not a double newline sequence, we typically merge.

                // Simplified Newline Logic for our overrides:
                // If UAX split on `\n` (meaning the segment *ends* with `\n`), we need to decide if it's a hard split or soft wrap.

                // Case 1: Double Newline (`\n\n`) -> Paragraph break -> ALWAYS Split.
                // We check if the text *up to* the split point ends with `\n\n`.
                if text[..idx].ends_with("\n\n") {
                    return true;
                }

                // Case 2: Partial Double Newline?
                // If segment ends with `\n` and NEXT char is `\n`, UAX puts boundary *between* them?
                // If we split at `\n` and the NEXT char is `\n`, it means we are in the middle of `\n\n`.
                // In that case, we should probably NOT split yet, but wait for the second `\n`.
                // So if next is `\n`, we SUPPRESS the split here.
                if next_char == Some('\n') {
                    return false;
                }

                // Case 3: Single Newline (`\n`) -> Soft wrap -> SUPPRESS split.
                // Unless it's EOF or something.
                if text[..idx].ends_with('\n') && !text[..idx].ends_with("\n\n") {
                    return false;
                }

                // If none of the above (e.g. `\n\n` ending), we allow split.
                return true;
            }
            _ => {}
        }

        // Default: If UAX says split, and no override triggered, we split.
        // Exception: Check for basic period handling.
        // UAX handles "Mr. Smith" correctly usually.
        // "ver.1.0" -> UAX handles.
        true
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
    fn test_split_consecutive_punctuation() {
        // "Hello。。World!?" should be split into 2 sentences based on heuristics.
        let text = "Hello。。World!?";
        let sentences = SentenceSplitter::split(text, &[]);

        assert!(sentences.len() >= 2);
        assert!(sentences[0].text.contains("Hello"));
        assert!(sentences.last().unwrap().text.contains("World"));
    }

    #[test]
    fn test_split_no_space_exclamation() {
        let text = "すごい！！本当に！？";
        let sentences = SentenceSplitter::split(text, &[]);
        // UAX splits after "！！".
        // Heuristic: "！！" followed by "本" (no space) -> Suppress split.
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "すごい！！本当に！？");
    }

    #[test]
    fn test_split_with_space_exclamation() {
        let text = "すごい！！ 本当に！？";
        // UAX splits after "！！ " (includes space in segment).
        // Heuristic: "！！ " followed by "本" -> Space checks out -> Keep split.
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 2);
        // Note: UAX segment includes trailing whitespace usually.
        assert_eq!(sentences[0].text, "すごい！！ ");
        assert_eq!(sentences[1].text, "本当に！？");
    }

    #[test]
    fn test_split_newlines() {
        let text = "Line1.\nLine2.\n\nParagraph2.";
        // "Line1.\n" -> merge (single newline)
        // "Line2.\n\n" -> split (double newline)

        let sentences = SentenceSplitter::split(text, &[]);

        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "Line1.\nLine2.\n\n");
        assert_eq!(sentences[1].text, "Paragraph2.");
    }

    #[test]
    fn test_split_english_mixed() {
        let text = "This is ver.1.0. Please visit example.com.";
        let sentences = SentenceSplitter::split(text, &[]);

        // "example.com." -> "example.com."
        // Expect 2 sentences.
        assert_eq!(sentences.len(), 2);
        assert!(sentences[0].text.starts_with("This is"));
        assert!(sentences[1].text.contains("Please visit"));
    }

    #[test]
    fn test_split_japanese_kuten() {
        let text = "こんにちは。元気？";
        // UAX splits.
        // Heuristic: 。 always splits.
        let sentences = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "こんにちは。");
        assert_eq!(sentences[1].text, "元気？");
    }

    #[test]
    fn test_split_yahoo_japan() {
        let text = "Yahoo! JAPAN"; // Split expected due to space
        let sentences_space = SentenceSplitter::split(text, &[]);
        assert_eq!(sentences_space.len(), 2);

        let text_no_space = "Yahoo!JAPAN"; // No split expected
        let sentences_no_space = SentenceSplitter::split(text_no_space, &[]);
        assert_eq!(sentences_no_space.len(), 1);
    }
}
