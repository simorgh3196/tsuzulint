//! Concrete native rule implementations.
//!
//! Every rule exposes a `pub static` singleton that the registry can wire up.

pub mod ja_no_mixed_period;
pub mod max_kanji_continuous_len;
pub mod max_ten;
pub mod no_doubled_joshi;
pub mod no_exclamation_question_mark;
pub mod no_hankaku_kana;
pub mod no_mixed_zenkaku_hankaku_alphabet;
pub mod no_nfd;
pub mod no_todo;
pub mod no_zero_width_spaces;
pub mod sentence_length;

use super::rule_trait::Rule;

/// All rules that the binary knows about at link time.
///
/// Ordering is irrelevant — the registry keys by name.
pub fn all() -> &'static [&'static dyn Rule] {
    static ALL: &[&'static dyn Rule] = &[
        &ja_no_mixed_period::RULE,
        &max_kanji_continuous_len::RULE,
        &max_ten::RULE,
        &no_doubled_joshi::RULE,
        &no_exclamation_question_mark::RULE,
        &no_hankaku_kana::RULE,
        &no_mixed_zenkaku_hankaku_alphabet::RULE,
        &no_nfd::RULE,
        &no_todo::RULE,
        &no_zero_width_spaces::RULE,
        &sentence_length::RULE,
    ];
    ALL
}
