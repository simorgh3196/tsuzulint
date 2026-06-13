//! `arabic-kanji-numbers` — enforce correct use of Arabic vs. kanji numerals in Japanese prose
//! (JTF style guide rule 2.2.2).
//!
//! A **surface** rule (JA-only).  It is a conservative port of the JTF-style rule that ships
//! inside `textlint-rule-preset-ja-technical-writing` as `jtfRules["2.2.2.算用数字と漢数字の使い分け"]`.
//!
//! # Two classes of violation
//!
//! 1. **Kanji → should be Arabic** (`kanji_to_arabic`): countable quantities expressed with
//!    kanji numerals that convention says should use Arabic digits.  Patterns (exact mirror of
//!    JTF 2.2.2):
//!    - `{kanji}つ` (counting things, e.g. `三つ` → `3つ`), with exceptions
//!    - `{kanji}回`  (occurrences/rounds)
//!    - `{kanji}か月` (months elapsed)
//!    - `{kanji}番目` (ordinal)
//!    - `{kanji}進法` (numeral system, e.g. 十進法)
//!    - `{kanji}次元` (dimension, e.g. 三次元)
//!    - `第{kanji}章` / `第{kanji}節` (chapter/section ordinals)
//!    - `{kanji}[兆億万]` (large-scale counts), ignoring `数…` / `何…` approximations
//!
//! 2. **Arabic → should be kanji** (`arabic_to_kanji`): Arabic digits in conventional
//!    idiomatic expressions.  Patterns (exact mirror of JTF 2.2.2):
//!    - `世界{1}` (世界一)
//!    - `{1}時的` (一時的)
//!    - `{1}部分` (一部分)
//!    - `第{3}者` (第三者)
//!    - `{1}種` (not followed by `類`, and not preceded by another digit)
//!    - `{1}部の` (一部の)
//!    - `{1}番に` (一番に)
//!    - `数{10+}倍` / `数{10+}[兆億万]` / `数{10+}年` (approximate multiples)
//!    - `{N}次関数` (polynomial degree — should be kanji, e.g. 二次関数)
//!    - `{5}大陸` (五大陸)
//!
//! # Divergence / deferred
//!
//! - **No autofix** — the upstream rule ships an Arabic↔kanji converter; we are report-only
//!   per project policy.  The suggested fix is embedded in the diagnostic message.
//! - `japanese-numerals-to-number` conversion and `_num2ja` conversion are **not** implemented
//!   in messages; messages show the matched text as a human-readable hint.
//! - The kanji numeral set recognised is: `一二三四五六七八九十壱弐参拾百〇` (same as upstream).
//! - `{kanji}回` is ported exactly; `{kanji}[兆億万]` ignores `数/何` prefixes.
//! - Rules that would require morphological context to distinguish (`1種` not in `11種`) are
//!   handled conservatively: `{1}種` checks for a non-digit preceding character.

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "arabic-kanji-numbers";

/// Flags incorrect use of Arabic vs. kanji numerals (JTF style guide 2.2.2).
pub struct ArabicKanjiNumbers {
    meta: RuleMeta,
}

impl ArabicKanjiNumbers {
    /// Construct the rule (no configurable options).
    pub fn new() -> Self {
        ArabicKanjiNumbers {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
        }
    }
}

impl Default for ArabicKanjiNumbers {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for ArabicKanjiNumbers {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();

        // Collect (byte_offset, char) pairs for indexed look-around.
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let n = chars.len();

        // --- Class 1: kanji numerals that should be Arabic ---
        check_kanji_to_arabic(text, &chars, n, base, cx);

        // --- Class 2: Arabic digits that should be kanji ---
        check_arabic_to_kanji(text, &chars, n, base, cx);
    }
}

// ---------------------------------------------------------------------------
// Kanji numeral characters
// ---------------------------------------------------------------------------

/// Returns `true` if `c` is one of the kanji numeral characters used in the upstream rule.
fn is_kanji_numeral(c: char) -> bool {
    matches!(
        c,
        '一' | '二'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
            | '壱'
            | '弐'
            | '参'
            | '拾'
            | '百'
            | '〇'
    )
}

// ---------------------------------------------------------------------------
// Class 1 helpers: kanji → Arabic
// ---------------------------------------------------------------------------

/// Scan `chars` for kanji numeral runs followed by specific suffixes that should use Arabic.
fn check_kanji_to_arabic(
    text: &str,
    chars: &[(usize, char)],
    n: usize,
    base: u32,
    cx: &mut Context,
) {
    let mut i = 0;
    while i < n {
        let (off, c) = chars[i];

        // --- `第{kanji}章` / `第{kanji}節` ---
        if c == '第'
            && let Some((run_end_i, _)) = kanji_run_after(chars, i + 1, n)
            && let Some((suff_off, suff_ch)) = chars.get(run_end_i)
            && matches!(suff_ch, '章' | '節')
        {
            let end = suff_off + suff_ch.len_utf8();
            let snippet = text.get(off..end).unwrap_or("");
            cx.report(abs_span(base, off, end), kanji_to_arabic_msg(snippet));
            i = run_end_i + 1;
            continue;
        }

        // All remaining class-1 patterns start with a kanji numeral run.
        if !is_kanji_numeral(c) {
            i += 1;
            continue;
        }

        // Collect the run.
        let run_start_off = off;
        let mut run_end_i = i;
        while run_end_i + 1 < n && is_kanji_numeral(chars[run_end_i + 1].1) {
            run_end_i += 1;
        }
        let suffix_i = run_end_i + 1;

        // Check `数/何` prefix — these are approximations, skip them.
        let has_approx_prefix = i > 0 && matches!(chars[i - 1].1, '数' | '何');

        if let Some((suff_off, suff_ch)) = chars.get(suffix_i) {
            match suff_ch {
                // `{kanji}[兆億万]` — but ignore `数…` / `何…` prefixes
                '兆' | '億' | '万' if !has_approx_prefix => {
                    let end = suff_off + suff_ch.len_utf8();
                    let snippet = text.get(run_start_off..end).unwrap_or("");
                    cx.report(
                        abs_span(base, run_start_off, end),
                        kanji_to_arabic_msg(snippet),
                    );
                    i = suffix_i + 1;
                    continue;
                }

                // `{kanji}つ` — with exceptions
                'つ' => {
                    let end = suff_off + suff_ch.len_utf8();
                    if !is_koto_exception(chars, suffix_i, n) {
                        let snippet = text.get(run_start_off..end).unwrap_or("");
                        cx.report(
                            abs_span(base, run_start_off, end),
                            kanji_to_arabic_msg(snippet),
                        );
                    }
                    i = suffix_i + 1;
                    continue;
                }

                // `{kanji}回`
                '回' => {
                    let end = suff_off + suff_ch.len_utf8();
                    let snippet = text.get(run_start_off..end).unwrap_or("");
                    cx.report(
                        abs_span(base, run_start_off, end),
                        kanji_to_arabic_msg(snippet),
                    );
                    i = suffix_i + 1;
                    continue;
                }

                // `{kanji}か月`
                'か' if chars.get(suffix_i + 1).map(|(_, c)| *c) == Some('月') => {
                    let (eo, ec) = chars[suffix_i + 1];
                    let end = eo + ec.len_utf8();
                    let snippet = text.get(run_start_off..end).unwrap_or("");
                    cx.report(
                        abs_span(base, run_start_off, end),
                        kanji_to_arabic_msg(snippet),
                    );
                    i = suffix_i + 2;
                    continue;
                }

                // `{kanji}番目`
                '番' if chars.get(suffix_i + 1).map(|(_, c)| *c) == Some('目') => {
                    let (eo, ec) = chars[suffix_i + 1];
                    let end = eo + ec.len_utf8();
                    let snippet = text.get(run_start_off..end).unwrap_or("");
                    cx.report(
                        abs_span(base, run_start_off, end),
                        kanji_to_arabic_msg(snippet),
                    );
                    i = suffix_i + 2;
                    continue;
                }

                // `{kanji}進法`
                '進' if chars.get(suffix_i + 1).map(|(_, c)| *c) == Some('法') => {
                    let (eo, ec) = chars[suffix_i + 1];
                    let end = eo + ec.len_utf8();
                    let snippet = text.get(run_start_off..end).unwrap_or("");
                    cx.report(
                        abs_span(base, run_start_off, end),
                        kanji_to_arabic_msg(snippet),
                    );
                    i = suffix_i + 2;
                    continue;
                }

                // `{kanji}次元`
                '次' if chars.get(suffix_i + 1).map(|(_, c)| *c) == Some('元') => {
                    let (eo, ec) = chars[suffix_i + 1];
                    let end = eo + ec.len_utf8();
                    let snippet = text.get(run_start_off..end).unwrap_or("");
                    cx.report(
                        abs_span(base, run_start_off, end),
                        kanji_to_arabic_msg(snippet),
                    );
                    i = suffix_i + 2;
                    continue;
                }

                _ => {}
            }
        }

        i += 1;
    }
}

/// Returns `true` if the `つ` at `suffix_i` is part of an idiomatic exception that should
/// remain kanji (upstream `ignoreWhenMatched` patterns for `{kanji}つ`).
///
/// Upstream exceptions:
/// - `[一二三四五六七八九]つ(返事|子|ひとつ|星|編|葉|橋|と[無な]い|に一つ)` — e.g. 一つ返事, 二つ子
/// - `(ただ|唯|[女男]手|穴|瓜|馬鹿の)[一二]つ` — e.g. ただ一つ, 穴一つ
fn is_koto_exception(chars: &[(usize, char)], suffix_i: usize, n: usize) -> bool {
    // Look at what comes after the つ (suffix_i is the つ position).
    let after1 = chars.get(suffix_i + 1).map(|(_, c)| *c);
    let after2 = chars.get(suffix_i + 2).map(|(_, c)| *c);
    let after3 = chars.get(suffix_i + 3).map(|(_, c)| *c);

    // Suffix patterns: 返事, 子, ひとつ, 星, 編, 葉, 橋, とない/とない, に一つ
    if matches!(after1, Some('返')) && matches!(after2, Some('事')) {
        return true;
    }
    if matches!(after1, Some('子')) {
        return true;
    }
    if matches!(after1, Some('ひ')) && matches!(after2, Some('と')) && matches!(after3, Some('つ'))
    {
        return true;
    }
    if matches!(after1, Some('星' | '編' | '葉' | '橋')) {
        return true;
    }
    // とない / となない — "とない" is "と無い"
    if matches!(after1, Some('と'))
        && matches!(after2, Some('無' | 'な'))
        && matches!(after3, Some('い'))
    {
        return true;
    }
    // に一つ
    if matches!(after1, Some('に')) && matches!(after2, Some('一')) && matches!(after3, Some('つ'))
    {
        return true;
    }

    // Prefix patterns: look before the kanji run.
    // The kanji run ends just before suffix_i; we check a few chars back.
    // For patterns like `ただ一つ` we need chars before the kanji run start.
    // We walk backwards from suffix_i - 1 (the last kanji) to see what's further back.
    // `suffix_i` is the つ; `suffix_i - 1` must be the last kanji char.
    // The prefix chars are those before the start of the kanji run.
    // To find the run start: walk back from suffix_i - 1 while is_kanji_numeral.
    let mut run_start = suffix_i.saturating_sub(1);
    while run_start > 0 && is_kanji_numeral(chars[run_start].1) {
        run_start -= 1;
    }
    // run_start now points at the char BEFORE the run (or at the first char of the run if idx=0).
    // Adjust: if chars[run_start] is not kanji, the run starts at run_start+1.
    let run_start = if is_kanji_numeral(chars[run_start].1) {
        run_start
    } else {
        run_start + 1
    };

    // Only [一二] can appear in the prefix-pattern exceptions.
    let kanji_is_single_one_or_two = suffix_i.saturating_sub(1) == run_start
        && matches!(chars.get(run_start).map(|(_, c)| *c), Some('一' | '二'));

    if kanji_is_single_one_or_two && run_start > 0 {
        let p1 = chars.get(run_start - 1).map(|(_, c)| *c);
        let p2 = run_start
            .checked_sub(2)
            .and_then(|i| chars.get(i))
            .map(|(_, c)| *c);
        let p3 = run_start
            .checked_sub(3)
            .and_then(|i| chars.get(i))
            .map(|(_, c)| *c);

        // `ただ一つ` / `唯一つ`
        if matches!(p1, Some('だ')) && matches!(p2, Some('た')) {
            return true;
        }
        if matches!(p1, Some('唯')) {
            return true;
        }
        // `女手一つ` / `男手一つ`
        if matches!(p1, Some('手')) && matches!(p2, Some('女' | '男')) {
            return true;
        }
        // `穴一つ` / `瓜一つ` / `馬鹿の一つ`
        if matches!(p1, Some('穴' | '瓜')) {
            return true;
        }
        if matches!(p1, Some('の')) && matches!(p2, Some('鹿')) && matches!(p3, Some('馬')) {
            return true;
        }
    }

    let _ = n; // suppress unused warning
    false
}

/// Returns `(first_idx_after_run, _)` for a kanji numeral run starting right at `from_i`.
/// Returns `None` if `chars[from_i]` is not a kanji numeral.
fn kanji_run_after(chars: &[(usize, char)], from_i: usize, n: usize) -> Option<(usize, usize)> {
    if from_i >= n || !is_kanji_numeral(chars[from_i].1) {
        return None;
    }
    let mut end_i = from_i;
    while end_i + 1 < n && is_kanji_numeral(chars[end_i + 1].1) {
        end_i += 1;
    }
    let end_off = chars[end_i].0 + chars[end_i].1.len_utf8();
    Some((end_i + 1, end_off)) // first index AFTER the run
}

// ---------------------------------------------------------------------------
// Class 2 helpers: Arabic → kanji
// ---------------------------------------------------------------------------

/// Check for Arabic digits in positions where the JTF guide demands kanji numerals.
fn check_arabic_to_kanji(
    text: &str,
    chars: &[(usize, char)],
    n: usize,
    base: u32,
    cx: &mut Context,
) {
    let mut i = 0;
    while i < n {
        let (off, c) = chars[i];

        // `世界{1}`
        if c == '世'
            && let Some((_, '界')) = chars.get(i + 1)
            && let Some((d_off, '1')) = chars.get(i + 2)
        {
            let end = d_off + 1;
            let snippet = text.get(off..end).unwrap_or("");
            cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
            i += 3;
            continue;
        }

        // `第{3}者`
        if c == '第'
            && let Some((_, '3')) = chars.get(i + 1)
            && let Some((_, '者')) = chars.get(i + 2)
        {
            let end = chars[i + 2].0 + '者'.len_utf8();
            let snippet = text.get(off..end).unwrap_or("");
            cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
            i += 3;
            continue;
        }

        // `数{10+}倍` / `数{10+}[兆億万]` / `数{10+}年`
        if c == '数'
            && let Some((_, '1')) = chars.get(i + 1)
        {
            // Collect the digit run starting at i+1 (must be '1' + at least one '0').
            let mut run_end = i + 1;
            while run_end + 1 < n && chars[run_end + 1].1 == '0' {
                run_end += 1;
            }
            // Must be at least "10" (two chars: '1' + at least one '0').
            if run_end > i + 1 {
                let next_i = run_end + 1;
                let suff = chars.get(next_i).map(|(_, c)| *c);
                if matches!(suff, Some('倍' | '兆' | '億' | '万' | '年')) {
                    let (so, sc) = chars[next_i];
                    let end = so + sc.len_utf8();
                    let snippet = text.get(off..end).unwrap_or("");
                    cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                    i = next_i + 1;
                    continue;
                }
            }
        }

        // Patterns that start with the digit itself.
        if c.is_ascii_digit() {
            // `{1}時的`
            if c == '1'
                && let Some((_, '時')) = chars.get(i + 1)
                && let Some((_, '的')) = chars.get(i + 2)
            {
                let end = chars[i + 2].0 + '的'.len_utf8();
                let snippet = text.get(off..end).unwrap_or("");
                cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                i += 3;
                continue;
            }

            // `{1}部分`
            if c == '1'
                && let Some((_, '部')) = chars.get(i + 1)
                && let Some((_, '分')) = chars.get(i + 2)
            {
                let end = chars[i + 2].0 + '分'.len_utf8();
                let snippet = text.get(off..end).unwrap_or("");
                cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                i += 3;
                continue;
            }

            // `{1}部の`
            if c == '1'
                && let Some((_, '部')) = chars.get(i + 1)
                && let Some((_, 'の')) = chars.get(i + 2)
            {
                let end = chars[i + 2].0 + 'の'.len_utf8();
                let snippet = text.get(off..end).unwrap_or("");
                cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                i += 3;
                continue;
            }

            // `{1}番に`
            if c == '1'
                && let Some((_, '番')) = chars.get(i + 1)
                && let Some((_, 'に')) = chars.get(i + 2)
            {
                let end = chars[i + 2].0 + 'に'.len_utf8();
                let snippet = text.get(off..end).unwrap_or("");
                cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                i += 3;
                continue;
            }

            // `{1}種` — not preceded by another digit, not followed by 類
            if c == '1' {
                let prev_is_digit = i > 0 && chars[i - 1].1.is_ascii_digit();
                if !prev_is_digit && let Some((_, '種')) = chars.get(i + 1) {
                    let followed_by_rui = chars.get(i + 2).map(|(_, c)| *c) == Some('類');
                    if !followed_by_rui {
                        let end = chars[i + 1].0 + '種'.len_utf8();
                        let snippet = text.get(off..end).unwrap_or("");
                        cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                        i += 2;
                        continue;
                    }
                }
            }

            // `{5}大陸`
            if c == '5'
                && let Some((_, '大')) = chars.get(i + 1)
                && let Some((_, '陸')) = chars.get(i + 2)
            {
                let end = chars[i + 2].0 + '陸'.len_utf8();
                let snippet = text.get(off..end).unwrap_or("");
                cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                i += 3;
                continue;
            }

            // `{N}次関数` — any digit(s) before 次関数
            {
                // Collect consecutive digit run starting at i.
                let mut run_end = i;
                while run_end + 1 < n && chars[run_end + 1].1.is_ascii_digit() {
                    run_end += 1;
                }
                let next_i = run_end + 1;
                if let Some((_, '次')) = chars.get(next_i)
                    && let Some((_, '関')) = chars.get(next_i + 1)
                    && let Some((_, '数')) = chars.get(next_i + 2)
                {
                    let end = chars[next_i + 2].0 + '数'.len_utf8();
                    let snippet = text.get(off..end).unwrap_or("");
                    cx.report(abs_span(base, off, end), arabic_to_kanji_msg(snippet));
                    i = next_i + 3;
                    continue;
                }
            }
        }

        i += 1;
    }
    let _ = n;
}

// ---------------------------------------------------------------------------
// Span helpers and messages
// ---------------------------------------------------------------------------

fn abs_span(base: u32, start: usize, end: usize) -> Span {
    Span::new(
        base.saturating_add(start as u32),
        base.saturating_add(end as u32),
    )
}

fn kanji_to_arabic_msg(snippet: &str) -> String {
    format!(
        "「{snippet}」の漢数字は算用数字で書いてください。\n\n\
         数量を表現し、数を数えられるものは算用数字を使用します。\
         任意の数に置き換えても通用する語句がこれに該当します。"
    )
}

fn arabic_to_kanji_msg(snippet: &str) -> String {
    format!(
        "「{snippet}」の算用数字は漢数字で書いてください。\n\n\
         慣用的表現、熟語、概数、固有名詞、副詞など、\
         漢数字を使用することが一般的な語句では漢数字を使います。"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    fn rule() -> ArabicKanjiNumbers {
        ArabicKanjiNumbers::new()
    }

    // --- Class 1: kanji → Arabic ---

    #[test]
    fn flags_kanji_tsu() {
        // 三つ → 3つ
        let diags = diagnose(&rule(), "三つ食べた\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("算用数字"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_kanji_kai() {
        // 三回 → 3回
        let diags = diagnose(&rule(), "三回やった\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_kanji_kagetsu() {
        // 三か月 → 3か月
        let diags = diagnose(&rule(), "三か月かかる\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_kanji_banme() {
        // 三番目 → 3番目
        let diags = diagnose(&rule(), "三番目に\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_kanji_shinhou() {
        // 十進法 → 10進法
        let diags = diagnose(&rule(), "十進法とは\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_kanji_jigen() {
        // 三次元 → 3次元
        let diags = diagnose(&rule(), "三次元空間\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_dai_kanji_sho() {
        // 第三章 → 第3章
        let diags = diagnose(&rule(), "第三章を読む\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_dai_kanji_setsu() {
        // 第一節 → 第1節
        let diags = diagnose(&rule(), "第一節の内容\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_kanji_large_unit() {
        // 三億 → 3億
        let diags = diagnose(&rule(), "三億円\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn allows_approx_prefix_suu() {
        // 数十億 — approximation, not flagged
        assert!(
            diagnose(&rule(), "数十億円\n").is_empty(),
            "数十億 must not be flagged"
        );
    }

    #[test]
    fn allows_approx_prefix_nani() {
        // 何百万 — approximation
        assert!(
            diagnose(&rule(), "何百万回も\n").is_empty(),
            "何百万 must not be flagged"
        );
    }

    #[test]
    fn allows_hitotsu_kanji_exceptions() {
        // 一つ子 — idiomatic, allowed
        assert!(
            diagnose(&rule(), "一つ子の\n").is_empty(),
            "一つ子 is exception"
        );
        // 一つ返事 — idiomatic
        assert!(
            diagnose(&rule(), "一つ返事で\n").is_empty(),
            "一つ返事 is exception"
        );
    }

    // --- Class 2: Arabic → kanji ---

    #[test]
    fn flags_ichijiteki() {
        // 1時的 → 一時的
        let diags = diagnose(&rule(), "1時的な措置\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("漢数字"), "{}", diags[0].message);
    }

    #[test]
    fn flags_ichibunsho() {
        // 1部分 → 一部分
        let diags = diagnose(&rule(), "1部分だけ\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_daisansha() {
        // 第3者 → 第三者
        let diags = diagnose(&rule(), "第3者に\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_isshu() {
        // 1種 → 一種 (not preceded by digit, not followed by 類)
        let diags = diagnose(&rule(), "1種の\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn allows_isshu_rui() {
        // 1種類 — not flagged (followed by 類)
        assert!(
            diagnose(&rule(), "1種類の\n").is_empty(),
            "1種類 must not be flagged"
        );
    }

    #[test]
    fn allows_isshu_preceded_by_digit() {
        // 11種 — not flagged (preceded by digit)
        assert!(
            diagnose(&rule(), "11種の\n").is_empty(),
            "11種 must not be flagged"
        );
    }

    #[test]
    fn flags_ichibu_no() {
        // 1部の → 一部の
        let diags = diagnose(&rule(), "1部の人\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_ichiban_ni() {
        // 1番に → 一番に
        let diags = diagnose(&rule(), "1番に来た\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_go_tairiku() {
        // 5大陸 → 五大陸
        let diags = diagnose(&rule(), "5大陸を\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_jikansuu() {
        // 2次関数 → 二次関数
        let diags = diagnose(&rule(), "2次関数の\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_suu_10bai() {
        // 数10倍 → 数十倍
        let diags = diagnose(&rule(), "数10倍に\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_suu_100nen() {
        // 数100年 → 数百年
        let diags = diagnose(&rule(), "数100年前\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn flags_sekai_ichi() {
        // 世界1 → 世界一
        let diags = diagnose(&rule(), "世界1の\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn clean_text_is_clean() {
        // Valid Arabic usage: countable quantity
        assert!(diagnose(&rule(), "3つ食べた\n").is_empty());
        assert!(diagnose(&rule(), "5回やった\n").is_empty());
        assert!(diagnose(&rule(), "第3章を読む\n").is_empty());
        // Valid kanji usage: idiomatic
        assert!(diagnose(&rule(), "一時的な措置\n").is_empty());
        assert!(diagnose(&rule(), "第三者に\n").is_empty());
        assert!(diagnose(&rule(), "世界一の\n").is_empty());
        assert!(diagnose(&rule(), "五大陸を\n").is_empty());
        assert!(diagnose(&rule(), "二次関数\n").is_empty());
    }
}
