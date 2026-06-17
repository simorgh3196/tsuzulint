//! `ja-unnatural-alphabet` — flag isolated alphabet characters that are almost certainly IME
//! input errors in Japanese text.
//!
//! A **surface** rule (JA-only). It detects a single half- or full-width Latin letter that
//! appears **between two Japanese characters**, following the same heuristic as the upstream
//! `textlint-rule-ja-unnatural-alphabet`.
//!
//! # Algorithm
//!
//! The pattern is: `{Japanese char}{single alphabet char}{Japanese char}`.  "Japanese char"
//! means anything in the CJK/Kana/punctuation ranges used by `japaneseRegExp` upstream.
//!
//! # Default allow-list (matches upstream defaults + `allowCommonCase`)
//!
//! - Vowels: `a i u e o` and their full-width equivalents `ａ ｉ ｕ ｅ ｏ`
//! - `n` / `ｎ` (often intended as `ん`)
//! - All uppercase half-width letters `A-Z` (likely intentional abbreviations)
//! - All uppercase full-width letters `Ａ-Ｚ`
//! - Common patterns with `[a-z]言語` (e.g. C言語), `[x-z]座標`, `[x-z]軸`, `Eメール`
//!   — these are excluded by checking the characters around the match rather than by
//!   accepting the matched letter wholesale.
//!
//! # Conservative choices / divergence from upstream
//!
//! - **No `allow` config option** — options are accepted for forward-compatibility but
//!   currently ignored; the hard-coded defaults already cover the upstream default set.
//! - **No autofix** — report-only, per project policy.
//! - The upstream rule uses `matchCaptureGroupAll` from `match-index` and separate `matchPatterns`
//!   for the allow-list.  We re-implement equivalently in Rust using `char_indices` scanning.

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-unnatural-alphabet";

/// Flags isolated alphabet characters that are almost certainly IME input errors.
pub struct JaUnnaturalAlphabet {
    meta: RuleMeta,
}

impl JaUnnaturalAlphabet {
    /// Construct the rule (no configurable options in this v1).
    pub fn new() -> Self {
        JaUnnaturalAlphabet {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
        }
    }
}

impl Default for JaUnnaturalAlphabet {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaUnnaturalAlphabet {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();

        // Collect (byte_offset, char) pairs so we can look at neighbours.
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let n = chars.len();

        for i in 1..n.saturating_sub(1) {
            let (off, ch) = chars[i];
            if !is_alphabet(ch) {
                continue;
            }
            // Neighbour must both be Japanese characters.
            let prev_ch = chars[i - 1].1;
            let next_ch = chars[i + 1].1;
            if !is_japanese(prev_ch) || !is_japanese(next_ch) {
                continue;
            }
            // Apply the allow-list.
            if is_allowed(ch, next_ch) {
                continue;
            }
            let start = base.saturating_add(off as u32);
            let end = base.saturating_add((off + ch.len_utf8()) as u32);
            cx.report(Span::new(start, end), message(ch));
        }
    }
}

/// Returns a Japanese-language diagnostic message for the offending character.
fn message(ch: char) -> String {
    format!(
        "不自然なアルファベットがあります: {ch}\n\n\
         IMEの入力ミスの可能性があります。意図した文字であれば無視してください。"
    )
}

// ---------------------------------------------------------------------------
// Character-class helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `c` is a half-width or full-width Latin letter.
fn is_alphabet(c: char) -> bool {
    c.is_ascii_alphabetic() || ('\u{FF41}'..='\u{FF5A}').contains(&c) // ａ-ｚ
        || ('\u{FF21}'..='\u{FF3A}').contains(&c) // Ａ-Ｚ
}

/// Returns `true` if `c` is in the Japanese character ranges used by the upstream rule
/// (`japaneseRegExp`): CJK Unified Ideographs, Hiragana, Katakana, CJK Extension A/B,
/// Compatibility Ideographs, Halfwidth/Fullwidth Forms, various JP punctuation.
fn is_japanese(c: char) -> bool {
    matches!(c,
        // CJK misc
        '々' | '〇' | '〻' |
        // CJK Extension A
        '\u{3400}'..='\u{4DBF}' |
        // CJK Unified Ideographs
        '\u{4E00}'..='\u{9FFF}' |
        // CJK Compatibility Ideographs
        '\u{F900}'..='\u{FAFF}' |
        // Hiragana + Katakana + prolonged sound mark + punctuation
        'ぁ'..='ん' | 'ァ'..='ヶ' | 'ー' |
        // Japanese punctuation used in prose
        '。' | '、' | '・' | '−' |
        // Halfwidth and Fullwidth Forms (includes full-width ASCII, half-width Kana)
        '\u{FF00}'..='\u{FFEF}'
    )
}

/// Returns `true` if the alphabet character `ch` followed by `next` is on the allow-list and
/// should **not** be flagged.
///
/// Allow-list (mirrors upstream defaults + `allowCommonCase`):
/// - Vowels (half- and full-width): a i u e o  / ａ ｉ ｕ ｅ ｏ
/// - `n` / `ｎ` (often intended as `ん`)
/// - All uppercase half-width: A-Z  (likely intentional abbreviations)
/// - All uppercase full-width: Ａ-Ｚ
/// - `[a-zA-Z]言語` pattern (e.g. C言語, c言語) — allowed regardless of next char
/// - `[x-zX-Z]座標` / `[x-zX-Z]軸` pattern — x/y/z axis notation
/// - `E`/`ｅ` before `メール` — "Eメール"
fn is_allowed(ch: char, next: char) -> bool {
    // All uppercase letters — intentional abbreviations (upstream: `"/[A-Z]/"`)
    if ch.is_ascii_uppercase() || ('\u{FF21}'..='\u{FF3A}').contains(&ch) {
        return true;
    }

    let lower = to_ascii_lower(ch);

    // Vowels + n
    if matches!(lower, 'a' | 'i' | 'u' | 'e' | 'o' | 'n') {
        return true;
    }

    // allowCommonCase patterns: `[a-zA-Z]言語`, `[x-z]座標`, `[x-z]軸`
    // We check `next` because the pattern is {JP}{alpha}{JP} and the third char is `next`.
    // "言語" starts with '言', "座標" with '座', "軸" is '軸', "メール" with 'メ'.
    if next == '言' {
        // [a-zA-Z]言語 — any single letter before 言 is fine (C言語, R言語, …)
        return true;
    }
    if matches!(lower, 'x' | 'y' | 'z') && matches!(next, '座' | '軸') {
        return true;
    }

    false
}

/// Convert a half-width or full-width Latin letter to its ASCII lowercase.
/// Returns the char unchanged if it is not Latin.
fn to_ascii_lower(c: char) -> char {
    if c.is_ascii_alphabetic() {
        return c.to_ascii_lowercase();
    }
    // Full-width a-z: U+FF41..U+FF5A
    if ('\u{FF41}'..='\u{FF5A}').contains(&c) {
        return (b'a' + (c as u32 - 0xFF41) as u8) as char;
    }
    // Full-width A-Z: U+FF21..U+FF3A
    if ('\u{FF21}'..='\u{FF3A}').contains(&c) {
        return (b'a' + (c as u32 - 0xFF21) as u8) as char;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    fn rule() -> JaUnnaturalAlphabet {
        JaUnnaturalAlphabet::new()
    }

    // --- cases that SHOULD be flagged ---

    #[test]
    fn flags_isolated_lowercase_between_japanese() {
        // リイr−ス (r between Japanese) — upstream example
        let diags = diagnose(&rule(), "リイrース\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("不自然なアルファベット"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_fullwidth_isolated_lowercase() {
        // ｋ (full-width k) surrounded by Japanese
        let diags = diagnose(&rule(), "対応でｋない\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_stray_b_between_japanese() {
        let diags = diagnose(&rule(), "あbい\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    // --- cases that should NOT be flagged ---

    #[test]
    fn allows_vowels() {
        assert!(diagnose(&rule(), "あaい\n").is_empty(), "a is allowed");
        assert!(diagnose(&rule(), "あiい\n").is_empty(), "i is allowed");
        assert!(diagnose(&rule(), "あuい\n").is_empty(), "u is allowed");
        assert!(diagnose(&rule(), "あeい\n").is_empty(), "e is allowed");
        assert!(diagnose(&rule(), "あoい\n").is_empty(), "o is allowed");
    }

    #[test]
    fn allows_n() {
        assert!(diagnose(&rule(), "あnい\n").is_empty(), "n is allowed");
    }

    #[test]
    fn allows_uppercase_letters() {
        // All uppercase are intentional abbreviations
        assert!(diagnose(&rule(), "あAい\n").is_empty(), "A is allowed");
        assert!(diagnose(&rule(), "あZい\n").is_empty(), "Z is allowed");
        assert!(diagnose(&rule(), "あBい\n").is_empty(), "B is allowed");
    }

    #[test]
    fn allows_fullwidth_uppercase() {
        // Full-width uppercase Ａ (U+FF21)
        assert!(
            diagnose(&rule(), "あＡい\n").is_empty(),
            "Ａ (full-width) is allowed"
        );
    }

    #[test]
    fn allows_letter_before_gengo() {
        // C言語, R言語 — common case allow
        assert!(
            diagnose(&rule(), "ではC言語を\n").is_empty(),
            "C言語 is allowed"
        );
        assert!(
            diagnose(&rule(), "ではr言語を\n").is_empty(),
            "r言語 is allowed"
        );
    }

    #[test]
    fn allows_xyz_axis_notation() {
        assert!(diagnose(&rule(), "はx軸で\n").is_empty(), "x軸 is allowed");
        assert!(
            diagnose(&rule(), "はy座標で\n").is_empty(),
            "y座標 is allowed"
        );
        assert!(diagnose(&rule(), "はz軸で\n").is_empty(), "z軸 is allowed");
    }

    #[test]
    fn alphabet_at_edge_not_sandwiched() {
        // 'r' at start/end — not sandwiched, must not fire
        assert!(diagnose(&rule(), "rはじまり\n").is_empty());
        assert!(diagnose(&rule(), "おわりr\n").is_empty());
    }

    #[test]
    fn non_japanese_neighbour_not_flagged() {
        // English word — neighbours are ASCII, not Japanese
        assert!(diagnose(&rule(), "API\n").is_empty());
        assert!(diagnose(&rule(), "hello world\n").is_empty());
    }

    #[test]
    fn span_points_at_the_offending_character() {
        // "あbい" — 'あ' is 3 bytes, 'b' is at byte 3, end at byte 4.
        let diags = diagnose(&rule(), "あbい\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            (diags[0].span.start, diags[0].span.end),
            (3, 4),
            "{diags:?}"
        );
    }
}
