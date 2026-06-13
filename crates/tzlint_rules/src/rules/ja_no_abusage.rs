//! `ja-no-abusage` — flag common Japanese misuses (誤用) from a fixed dictionary, reporting the
//! incorrect form and the recommended correct form. Ported from
//! `textlint-ja/textlint-rule-ja-no-abusage`.
//!
//! A **surface** rule (Japanese). Upstream ships both a `dictionary.ts` (morpheme-token-matched
//! entries) and a `dict/prh.yml` (surface substitution entries). This port implements:
//!
//! - **All non-regex prh.yml entries** as exact substring matches: 39 entries.
//! - **2 of 4 `dictionary.ts` entries** as exact surface matches (「を適応」→適用 and 「可変する」),
//!   which are safe without token-boundary checks.
//!
//! **Deferred** (not ported, documented below):
//! - The `ずらい/づらい` morpheme entries (entries 3 & 4 of dictionary.ts): they require
//!   confirming the preceding token is a verb 連用形, which demands morphology. Ported zero rather
//!   than risk false positives on e.g. standalone 「ずら」strings.
//! - All prh.yml entries whose `patterns` field is a regex (`/…/`): 15 entries. These need
//!   a regex engine and optional capture groups; porting them faithfully without that machinery
//!   would require hard-coding each specific alternative, which risks mis-coverage.
//!
//! Total ported: **41 entries** (39 prh.yml fixed-string + 2 dictionary.ts surface-safe).
//! Report-only (no autofix).

use tzlint_ast::morphology::Lang;
use tzlint_ast::{NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "ja-no-abusage";

/// A single dictionary entry: the misused phrase, the correct form, and the diagnostic message.
struct Entry {
    /// The misused surface substring (UTF-8) to search for.
    wrong: &'static str,
    /// Japanese diagnostic message, including the correct form.
    message: &'static str,
}

/// The ported dictionary. 41 entries total.
///
/// Sources:
/// - `dict/prh.yml` (fixed-string patterns only, 39 entries)
/// - `src/dictionary.ts` entries 1 and 2 (surface-safe without morphology, 2 entries)
static DICT: &[Entry] = &[
    // ── From src/dictionary.ts ──────────────────────────────────────────────────────────────────
    // Entry 1: を適応 → を適用
    Entry {
        wrong: "を適応",
        message: "「を適応」は「を適用」の誤用である可能性があります。適応 → 適用",
    },
    // Entry 2: 可変する → (no specific replacement; "可変" is not a verb in standard usage)
    Entry {
        wrong: "可変する",
        message: "「可変する」という使い方は適切ではありません。「可逆」と同じ使い方になります。\
可変は状態の形容であり、「変更できる」「可変長の」などの形で使ってください。",
    },
    // ── From dict/prh.yml (fixed-string patterns only) ─────────────────────────────────────────
    Entry {
        wrong: "値を返却する",
        message: "「値を返却する」は「値を返す」の誤用です。正しくは「値を返す」。",
    },
    Entry {
        wrong: "例外を補足",
        message: "「例外を補足」は「例外を捕捉」の誤用です。正しくは「例外を捕捉」。",
    },
    Entry {
        wrong: "似て異なるもの",
        message: "「似て異なるもの」は「似て非なるもの」の誤用です。正しくは「似て非なるもの」。",
    },
    Entry {
        wrong: "愛苦しい",
        message: "「愛苦しい」は「愛くるしい」の誤用です。正しくは「愛くるしい」。",
    },
    Entry {
        wrong: "悪どい",
        message: "「悪どい」は「あくどい」の誤用です。正しくは「あくどい」。",
    },
    Entry {
        wrong: "転稼",
        message: "「転稼」は「転嫁」の誤用です。正しくは「転嫁」。",
    },
    Entry {
        wrong: "こんにちわ",
        message: "「こんにちわ」は「こんにちは」の誤用です。正しくは「こんにちは」。",
    },
    Entry {
        wrong: "論戦を張る",
        message: "「論戦を張る」は「論陣を張る」の誤用です。正しくは「論陣を張る」。",
    },
    Entry {
        wrong: "寸暇を惜しまず",
        message: "「寸暇を惜しまず」は「寸暇を惜しんで」の誤用です。正しくは「寸暇を惜しんで」。",
    },
    Entry {
        wrong: "寸暇を置かず",
        message: "「寸暇を置かず」は「間を置かず」の誤用です。正しくは「間を置かず」。",
    },
    Entry {
        wrong: "一つ返事",
        message: "「一つ返事」は「二つ返事」の誤用です。正しくは「二つ返事」。",
    },
    Entry {
        wrong: "怒り心頭に達する",
        message: "「怒り心頭に達する」は「怒り心頭に発する」の誤用です。正しくは「怒り心頭に発する」。",
    },
    Entry {
        wrong: "熱にうなされる",
        message: "「熱にうなされる」は「熱に浮かされる」の誤用です。正しくは「熱に浮かされる」。",
    },
    Entry {
        wrong: "やるせぬ",
        message: "「やるせぬ」は「やるせない」の誤用です。正しくは「やるせない」。",
    },
    Entry {
        wrong: "雪辱を晴らす",
        message: "「雪辱を晴らす」は「雪辱を果たす」の誤用です。正しくは「雪辱を果たす」。",
    },
    Entry {
        wrong: "うる覚え",
        message: "「うる覚え」は「うろ覚え」の誤用です。正しくは「うろ覚え」。",
    },
    Entry {
        wrong: "口先三寸",
        message: "「口先三寸」は「舌先三寸」の誤用です。正しくは「舌先三寸」。",
    },
    Entry {
        wrong: "舌の先の乾かぬ",
        message: "「舌の先の乾かぬ」は「舌の根の乾かぬ」の誤用です。正しくは「舌の根の乾かぬ」。",
    },
    Entry {
        wrong: "悪評さくさく",
        message: "「悪評さくさく」は「悪評高い」の誤用です。正しくは「悪評高い」。",
    },
    Entry {
        wrong: "悪評嘖嘖",
        message: "「悪評嘖嘖」は「悪評高い」の誤用です。正しくは「悪評高い」。",
    },
    Entry {
        wrong: "悪評嘖々",
        message: "「悪評嘖々」は「悪評高い」の誤用です。正しくは「悪評高い」。",
    },
    Entry {
        wrong: "器管",
        message: "「器管」は「器官」の誤用です。正しくは「器官」。",
    },
    Entry {
        wrong: "上や下への",
        message: "「上や下への」は「上を下への」の誤用です。正しくは「上を下への」。",
    },
    Entry {
        wrong: "上へ下への",
        message: "「上へ下への」は「上を下への」の誤用です。正しくは「上を下への」。",
    },
    Entry {
        wrong: "押しも押されぬ",
        message: "「押しも押されぬ」は「押しも押されもせぬ」の誤用です。正しくは「押しも押されもせぬ」。",
    },
    Entry {
        wrong: "掻き入れ",
        message: "「掻き入れ」は「書き入れ」の誤用です。正しくは「書き入れ」。",
    },
    Entry {
        wrong: "引くに引かれない",
        message: "「引くに引かれない」は「引くに引けない」の誤用です。正しくは「引くに引けない」。",
    },
    Entry {
        wrong: "死ぬに死なれない",
        message: "「死ぬに死なれない」は「死ぬに死ねない」の誤用です。正しくは「死ぬに死ねない」。",
    },
    Entry {
        wrong: "腹が煮えくり返る",
        message: "「腹が煮えくり返る」は「腸が煮えくり返る」の誤用です。正しくは「腸が煮えくり返る」。",
    },
    Entry {
        wrong: "胸先三寸",
        message: "「胸先三寸」は「胸三寸」の誤用です。正しくは「胸三寸」。",
    },
    Entry {
        wrong: "肝に据えかねる",
        message: "「肝に据えかねる」は「腹に据えかねる」の誤用です。正しくは「腹に据えかねる」。",
    },
    Entry {
        wrong: "へそを抱えて",
        message: "「へそを抱えて」は「腹を抱えて」の誤用です。正しくは「腹を抱えて」。",
    },
    Entry {
        wrong: "後ろ立て",
        message: "「後ろ立て」は「後ろ盾」の誤用です。正しくは「後ろ盾」。",
    },
    Entry {
        wrong: "絶対絶命",
        message: "「絶対絶命」は「絶体絶命」の誤用です。正しくは「絶体絶命」。",
    },
    Entry {
        wrong: "有頂点",
        message: "「有頂点」は「有頂天」の誤用です。正しくは「有頂天」。",
    },
    Entry {
        wrong: "一発即発",
        message: "「一発即発」は「一触即発」の誤用です。正しくは「一触即発」。",
    },
    Entry {
        wrong: "意気高々",
        message: "「意気高々」は「意気揚々」の誤用です。正しくは「意気揚々」。",
    },
    Entry {
        wrong: "青色吐息",
        message: "「青色吐息」は「青息吐息」の誤用です。正しくは「青息吐息」。",
    },
    Entry {
        wrong: "固定概念",
        message: "「固定概念」は「固定観念」の誤用です。正しくは「固定観念」。",
    },
    Entry {
        wrong: "目を止め",
        message: "「目を止め」は「目を留め」の誤用です。「止」は動作の停止、「留」は意識を向ける時に使います。正しくは「目を留め」。",
    },
    Entry {
        wrong: "睨みを効かせ",
        message: "「睨みを効かせ」は「睨みを利かせ」の誤用です。正しくは「睨みを利かせ」。",
    },
    Entry {
        wrong: "恩を着る",
        message: "「恩を着る」は「恩に着る」の誤用です。正しくは「恩に着る」。",
    },
    Entry {
        wrong: "脚光を集め",
        message: "「脚光を集め」は「脚光を浴び」の誤用です。正しくは「脚光を浴び」。",
    },
    Entry {
        wrong: "一同に会する",
        message: "「一同に会する」は「一堂に会する」の誤用です。正しくは「一堂に会する」。",
    },
];

/// Flags common Japanese misuses (誤用).
pub struct JaNoAbusage {
    meta: RuleMeta,
}

impl JaNoAbusage {
    /// Construct the rule (no options).
    pub fn new() -> Self {
        JaNoAbusage {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .for_language(Lang::JA),
        }
    }
}

impl Default for JaNoAbusage {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for JaNoAbusage {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let base = node.span().start;
        let text = node.text();

        for entry in DICT {
            let phrase_len = entry.wrong.len();
            let mut search_from = 0usize;
            while let Some(rel) = text[search_from..].find(entry.wrong) {
                let abs = search_from + rel;
                let abs_start = base.saturating_add(abs as u32);
                let abs_end = base.saturating_add((abs + phrase_len) as u32);
                cx.report(Span::new(abs_start, abs_end), entry.message);
                search_from = abs + phrase_len;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::diagnose;

    #[test]
    fn flags_tekiou() {
        let diags = diagnose(&JaNoAbusage::new(), "法律を適応する。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("適用"), "{}", diags[0].message);
    }

    #[test]
    fn flags_kahen_suru() {
        let diags = diagnose(&JaNoAbusage::new(), "長さを可変する。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("可変する"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_chikku_kaeshi() {
        let diags = diagnose(&JaNoAbusage::new(), "値を返却する関数。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("値を返す"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_konnichiwa() {
        let diags = diagnose(&JaNoAbusage::new(), "こんにちわ、世界。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("こんにちは"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_zettai_zetsumei() {
        let diags = diagnose(&JaNoAbusage::new(), "絶対絶命のピンチ。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("絶体絶命"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_koteigainen() {
        let diags = diagnose(&JaNoAbusage::new(), "固定概念にとらわれている。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("固定観念"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_kanou() {
        let diags = diagnose(&JaNoAbusage::new(), "悪評嘖々の製品。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("悪評高い"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn flags_uri_kae() {
        let diags = diagnose(&JaNoAbusage::new(), "うる覚えで書いた。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("うろ覚え"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn clean_text_is_not_flagged() {
        // Correct forms should not trigger.
        assert!(diagnose(&JaNoAbusage::new(), "こんにちは、絶体絶命の危機。\n").is_empty());
        assert!(diagnose(&JaNoAbusage::new(), "法律を適用する。\n").is_empty());
        assert!(diagnose(&JaNoAbusage::new(), "うろ覚えで書いた。\n").is_empty());
    }

    #[test]
    fn flags_span_is_tight_on_wrong_phrase() {
        // 「こんにちわ」is 15 bytes (5 chars × 3 bytes each in UTF-8).
        let diags = diagnose(&JaNoAbusage::new(), "こんにちわ、世界。\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        // The span should cover exactly the 5-character wrong phrase (bytes 0..15).
        assert_eq!(diags[0].span.start, 0, "span start");
        assert_eq!(diags[0].span.end, 15, "span end"); // 5 chars × 3 bytes
    }
}
