//! Shared, pure text helpers used by several rules. No AST, no panics.

/// Sentence-terminating delimiters (ASCII and full-width), delimiter-inclusive when splitting.
const SENTENCE_DELIMITERS: [char; 6] = ['.', '!', '?', '。', '！', '？'];

/// Split `text` into sentences on [`SENTENCE_DELIMITERS`].
///
/// Each sentence includes its terminating delimiter and has leading whitespace trimmed; empty
/// pieces are dropped. A trailing run with no delimiter is returned as a final sentence.
pub fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if SENTENCE_DELIMITERS.contains(&c) {
            let end = i + c.len_utf8();
            let sentence = text[start..end].trim_start();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = end;
        }
    }
    if start < text.len() {
        let tail = text[start..].trim_start();
        if !tail.is_empty() {
            sentences.push(tail);
        }
    }
    sentences
}

/// Collapse each `http://`/`https://` run to a single `・` so a long URL counts as one character
/// and the dots inside it are not treated as sentence boundaries.
pub fn strip_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("http://") || rest.starts_with("https://") {
            out.push('・');
            // Consume the URL run up to (but not including) the next whitespace or closing mark.
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '」' | '）' | ')' | '、' | '。'))
                .unwrap_or(rest.len());
            rest = &rest[end..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Whether `c` is a Han ideograph in CJK Unified (`U+4E00..=U+9FFF`) or Extension A
/// (`U+3400..=U+4DBF`). Deliberately narrow: the iteration mark `々` (U+3005), CJK Compatibility
/// Ideographs, and Extension B+ are excluded (preserve for parity — do not widen).
pub fn is_kanji(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// Whether `c` is in the half-width katakana block (`U+FF61..=U+FF9F`). This is the *whole* block,
/// so it also includes half-width punctuation/brackets/sound-marks (｡ ｢ ｣ ･ ｰ ﾞ ﾟ) — intentional;
/// do not tighten.
pub fn is_halfwidth_kana(c: char) -> bool {
    matches!(c as u32, 0xFF61..=0xFF9F)
}

/// Whether `c` is a full-width Latin letter (`Ａ–Ｚ` `U+FF21..=U+FF3A`, `ａ–ｚ` `U+FF41..=U+FF5A`).
pub fn is_fullwidth_alpha(c: char) -> bool {
    matches!(c as u32, 0xFF21..=0xFF3A | 0xFF41..=0xFF5A)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_sentences_basic() {
        assert_eq!(split_sentences("Hello. World!"), vec!["Hello.", "World!"]);
        assert_eq!(
            split_sentences("一文目。二文目。"),
            vec!["一文目。", "二文目。"]
        );
        // Trailing remainder with no delimiter is a sentence.
        assert_eq!(split_sentences("no terminator"), vec!["no terminator"]);
        // Leading whitespace is trimmed; empty pieces dropped.
        assert_eq!(split_sentences("a.  b."), vec!["a.", "b."]);
        assert!(split_sentences("").is_empty());
    }

    #[test]
    fn strip_urls_collapses_runs() {
        assert_eq!(
            strip_urls("see https://example.com/a.b here"),
            "see ・ here"
        );
        assert_eq!(strip_urls("（http://x.y）after"), "（・）after");
        // No URL → unchanged.
        assert_eq!(strip_urls("plain text"), "plain text");
        // A bare scheme word that is not http(s) is left alone.
        assert_eq!(strip_urls("ftp://x"), "ftp://x");
    }

    #[test]
    fn char_classes() {
        assert!(is_kanji('漢'));
        assert!(!is_kanji('々')); // iteration mark excluded
        assert!(!is_kanji('あ'));
        assert!(is_halfwidth_kana('ｱ'));
        assert!(is_halfwidth_kana('｡'));
        assert!(!is_halfwidth_kana('ア')); // full-width kana is fine
        assert!(is_fullwidth_alpha('Ａ'));
        assert!(is_fullwidth_alpha('ｚ'));
        assert!(!is_fullwidth_alpha('A'));
    }
}
