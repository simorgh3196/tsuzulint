//! `no-doubled-joshi` — flag a Japanese 助詞 (particle) repeated within a short same-sentence
//! window (e.g. 「〜の〜の」), which reads awkwardly.
//!
//! This is the first **morphology-dependent** built-in: it declares
//! [`with_morphology`](RuleMeta::with_morphology) for Japanese, so the engine runs it only when a
//! morphology table is available (a JA provider is injected). It reads the dictionary
//! part-of-speech to decide what is a 助詞 and groups repeats by surface + POS, then flags a pair
//! whose in-sentence gap is shorter than `min_interval`.
//!
//! Detection keys on the **IPADIC** POS literal `"助詞"` and IPADIC sub-type strings
//! (連体化 / 格助詞 / 接続助詞 / 並立助詞). A different tagset (e.g. a UniDic fused `"助詞-係助詞"`)
//! simply does not match, so the rule no-ops rather than mis-firing — a false negative, never a
//! false positive. Reconciling the exact POS strings with a real backend is a follow-up (M2j).

use std::collections::BTreeMap;

use serde_json::Value;
use tzlint_ast::morphology::{ArchivedMorphologyV1, ArchivedToken, FeatureKey, Lang};
use tzlint_ast::{ArchivedAst, NodeKind, Span};
use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

/// The rule id.
pub const ID: &str = "no-doubled-joshi";

const DEFAULT_MIN_INTERVAL: usize = 1;
const DEFAULT_SEPARATORS: [char; 7] = ['.', '．', '。', '?', '!', '？', '！'];
const DEFAULT_COMMAS: [char; 2] = ['、', '，'];
/// Bracket characters that each add 1 to the gap weight (legacy-fixed, not configurable).
const BRACKETS: [char; 6] = ['(', ')', '（', '）', '「', '」'];
/// Particles dropped in non-strict mode (surface, POS sub-type): a repeat of these is idiomatic.
const EXCEPTIONS: [(&str, &str); 3] = [("の", "連体化"), ("を", "格助詞"), ("て", "接続助詞")];

/// Flags a 助詞 doubled within a short same-sentence window.
pub struct NoDoubledJoshi {
    meta: RuleMeta,
    min_interval: usize,
    strict: bool,
    allow: Vec<String>,
    separator_characters: Vec<char>,
    comma_characters: Vec<char>,
}

impl NoDoubledJoshi {
    /// Construct with default options (`min_interval` 1, non-strict, no `allow`, the default
    /// separator/comma sets).
    pub fn new() -> Self {
        NoDoubledJoshi {
            meta: RuleMeta::new(
                ID,
                Severity::Warning,
                vec![NodeKind::PARAGRAPH, NodeKind::HEADING, NodeKind::TABLE_CELL],
            )
            .with_morphology(Lang::JA),
            min_interval: DEFAULT_MIN_INTERVAL,
            strict: false,
            allow: Vec::new(),
            separator_characters: DEFAULT_SEPARATORS.to_vec(),
            comma_characters: DEFAULT_COMMAS.to_vec(),
        }
    }

    /// Construct from config `options`, leniently (missing/wrong-typed values keep defaults):
    /// `min_interval` (integer), `strict` (bool), `allow` (array of surface strings),
    /// `separator_characters` / `comma_characters` (arrays of strings; the first char of each).
    pub fn from_options(options: &Value) -> Self {
        let mut rule = Self::new();
        if let Some(value) = options.get("min_interval").and_then(Value::as_u64) {
            // Fail open toward "never flag" on a 32-bit target rather than truncating.
            rule.min_interval = usize::try_from(value).unwrap_or(usize::MAX);
        }
        if let Some(strict) = options.get("strict").and_then(Value::as_bool) {
            rule.strict = strict;
        }
        if let Some(array) = options.get("allow").and_then(Value::as_array) {
            rule.allow = array
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        if let Some(chars) = char_list(options, "separator_characters") {
            rule.separator_characters = chars;
        }
        if let Some(chars) = char_list(options, "comma_characters") {
            rule.comma_characters = chars;
        }
        rule
    }
}

/// Parse an option that is an array of strings into the first char of each (skipping empties).
fn char_list(options: &Value, key: &str) -> Option<Vec<char>> {
    options.get(key).and_then(Value::as_array).map(|array| {
        array
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|s| s.chars().next())
            .collect()
    })
}

impl Default for NoDoubledJoshi {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoDoubledJoshi {
    fn meta(&self) -> &RuleMeta {
        &self.meta
    }

    fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
        let Some(table) = cx.morphology() else {
            return; // belt-and-suspenders over the engine's morphology gate
        };
        let ast = cx.ast();
        let base = node.span().start;
        let text = node.text();

        // 1. Collect this node's tokens, source-ordered. The `(node, surface.start)` emission order
        //    is a producer contract the builder does not enforce, so sort defensively.
        let mut tokens: Vec<&ArchivedToken> = cx.tokens_of(node.id()).collect();
        tokens.sort_by_key(|t| (t.surface().start, t.surface().end));

        // 2. Extract particles, skipping OOV guesses, `allow`ed surfaces, and (non-strict) the
        //    idiomatic exceptions の/を/て.
        let mut particles: Vec<Particle<'ast>> = Vec::new();
        for (idx, &tok) in tokens.iter().enumerate() {
            if tok.is_unknown() {
                continue;
            }
            if feature(tok, table, FeatureKey::POS) != Some("助詞") {
                continue;
            }
            let span = tok.surface();
            let surface = ast.text_of(span).unwrap_or("");
            if self.allow.iter().any(|allowed| allowed == surface) {
                continue;
            }
            let subtype = feature(tok, table, FeatureKey::POS_SUB_1);
            if !self.strict
                && EXCEPTIONS
                    .iter()
                    .any(|(s, st)| *s == surface && subtype == Some(*st))
            {
                continue;
            }
            let prev_word = idx
                .checked_sub(1)
                .and_then(|i| ast.text_of(tokens[i].surface()))
                .unwrap_or("");
            let key = match subtype {
                Some(st) => format!("{surface}:助詞.{st}"),
                None => format!("{surface}:助詞"),
            };
            particles.push(Particle {
                surface,
                key,
                start: span.start,
                end: span.end,
                tok_idx: idx,
                prev_word,
                subtype,
            });
        }

        // 3. Merge byte-contiguous particles into 連語 compounds (に+は → には), re-keyed by surface.
        let mut merged: Vec<Particle<'ast>> = Vec::new();
        for particle in particles {
            if let Some(last) = merged.last_mut()
                && last.end == particle.start
            {
                last.end = particle.end;
                last.surface = ast.text_of(Span::new(last.start, last.end)).unwrap_or("");
                last.key = format!("{}:連語", last.surface);
                last.subtype = Some("連語");
                continue;
            }
            merged.push(particle);
        }

        // 4+5. Group by (sentence index, key). The BTreeMap is deterministic and each group's Vec is
        //      source-ordered because `merged` is sorted by start.
        let sentence_idx = |start: u32| -> usize {
            let upto = start.saturating_sub(base) as usize;
            text.get(..upto)
                .unwrap_or("")
                .chars()
                .filter(|c| self.separator_characters.contains(c))
                .count()
        };
        let mut groups: BTreeMap<(usize, &str), Vec<usize>> = BTreeMap::new();
        for (i, particle) in merged.iter().enumerate() {
            groups
                .entry((sentence_idx(particle.start), particle.key.as_str()))
                .or_default()
                .push(i);
        }

        // 6. Flag adjacent same-key pairs whose in-sentence gap is under `min_interval`.
        let mut violations: Vec<(Span, String)> = Vec::new();
        for members in groups.values() {
            if members.len() < 2 {
                continue;
            }
            // Non-strict: a group made up only of 並立助詞 (parallel particles) is idiomatic.
            if !self.strict
                && members
                    .iter()
                    .all(|&i| merged[i].subtype == Some("並立助詞"))
            {
                continue;
            }
            for pair in members.windows(2) {
                let prev = &merged[pair[0]];
                let curr = &merged[pair[1]];
                if !self.strict && is_ka_douka(prev, curr, &tokens, ast) {
                    continue;
                }
                if self.interval(prev, curr, text, base) < self.min_interval {
                    violations.push((Span::new(curr.start, curr.end), self.message(prev, curr)));
                }
            }
        }

        for (span, message) in violations {
            cx.report(span, message);
        }
    }
}

impl NoDoubledJoshi {
    /// The weighted gap between two particles in the **same** sentence: 0 when they touch; otherwise
    /// each comma/bracket char in the source between them adds 1. (Separators need no handling here —
    /// two particles only reach this comparison when they share a sentence index, and by
    /// construction no separator can lie between same-sentence particles.)
    fn interval(&self, prev: &Particle, curr: &Particle, text: &str, base: u32) -> usize {
        if curr.start <= prev.end {
            return 0;
        }
        let lo = prev.end.saturating_sub(base) as usize;
        let hi = curr.start.saturating_sub(base) as usize;
        let mut weight = 0usize;
        for ch in text.get(lo..hi).unwrap_or("").chars() {
            if self.comma_characters.contains(&ch) || BRACKETS.contains(&ch) {
                weight += 1;
            }
        }
        weight
    }

    /// The Japanese diagnostic for a doubled particle (the second occurrence's surface names it).
    fn message(&self, prev: &Particle, curr: &Particle) -> String {
        format!(
            "一文に二回以上利用されている助詞 \"{name}\" がみつかりました。\n\n\
             次の助詞が連続しているため、文を読みにくくしています。\n\n\
             {prev_line}\n{curr_line}\n\n\
             同じ助詞を連続して利用しない、文の中で順番を入れ替える、文を分割するなどを検討してください。",
            name = curr.surface,
            prev_line = body_line(prev),
            curr_line = body_line(curr),
        )
    }
}

/// One extracted particle: its surface text + grouping `key` (surface + POS + sub-type), absolute
/// byte span, index into the source-sorted token list, the preceding token's surface (for the
/// message), and its POS sub-type.
struct Particle<'a> {
    surface: &'a str,
    key: String,
    start: u32,
    end: u32,
    tok_idx: usize,
    prev_word: &'a str,
    subtype: Option<&'a str>,
}

/// A message body line: `- {prev_word}"{surface}"`, or `- "{surface}"` at the start of the node.
fn body_line(particle: &Particle) -> String {
    if particle.prev_word.is_empty() {
        format!("- \"{}\"", particle.surface)
    } else {
        format!("- {}\"{}\"", particle.prev_word, particle.surface)
    }
}

/// Whether `prev`/`curr` are the two か of a かどうか pattern (token-precise: exactly one token
/// between them in the source stream, whose surface is どう) — skipped in non-strict mode.
fn is_ka_douka(
    prev: &Particle,
    curr: &Particle,
    tokens: &[&ArchivedToken],
    ast: &ArchivedAst,
) -> bool {
    prev.surface == "か"
        && curr.surface == "か"
        && curr.tok_idx == prev.tok_idx + 2
        && tokens
            .get(prev.tok_idx + 1)
            .and_then(|t| ast.text_of(t.surface()))
            == Some("どう")
}

/// Resolve a feature value (e.g. POS) of `token` against `table`, or `None`.
fn feature<'a>(
    token: &ArchivedToken,
    table: &'a ArchivedMorphologyV1,
    key: FeatureKey,
) -> Option<&'a str> {
    token
        .features(table)
        .find(|(k, _)| *k == key)
        .and_then(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{diagnose, diagnose_with_morphology};
    use tzlint_ast::NodeId;
    use tzlint_ast::morphology::{MorphologyBuilder, Tagset, TokenAttrs};

    /// Push a 助詞 token: surface `[start,end)`, POS=助詞, POS_SUB_1=`subtype`.
    fn joshi(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32, subtype: &str) {
        b.push_token(
            TokenAttrs {
                node,
                surface: Span::new(start, end),
                lang: Lang::JA,
                tagset: Tagset::IPADIC,
                flags: 0,
            },
            None,
            None,
            &[(FeatureKey::POS, "助詞"), (FeatureKey::POS_SUB_1, subtype)],
        );
    }

    /// Push a non-particle (noun) token at `[start,end)`.
    fn word(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
        b.push_token(
            TokenAttrs {
                node,
                surface: Span::new(start, end),
                lang: Lang::JA,
                tagset: Tagset::IPADIC,
                flags: 0,
            },
            None,
            None,
            &[(FeatureKey::POS, "名詞")],
        );
    }

    #[test]
    fn flags_doubled_particle() {
        // 私は彼は : は(係助詞) at 3..6 and 9..12, "彼" between (weight 0 < min 1) → flag the 2nd は.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "私は彼は", |pid, b| {
            word(b, pid, 0, 3); // 私
            joshi(b, pid, 3, 6, "係助詞"); // は
            word(b, pid, 6, 9); // 彼
            joshi(b, pid, 9, 12, "係助詞"); // は
        });
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("一文に二回以上利用されている助詞 \"は\""),
            "{}",
            diags[0].message
        );
        assert_eq!((diags[0].span.start, diags[0].span.end), (9, 12));
    }

    #[test]
    fn exception_no_is_not_flagged() {
        // の(連体化)×2 in non-strict mode → dropped at extraction → no diagnostic.
        let diags =
            diagnose_with_morphology(&NoDoubledJoshi::new(), "本のペンの色", |pid, b| {
                word(b, pid, 0, 3); // 本
                joshi(b, pid, 3, 6, "連体化"); // の
                word(b, pid, 6, 12); // ペン
                joshi(b, pid, 12, 15, "連体化"); // の
                word(b, pid, 15, 18); // 色
            });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn sentence_boundary_resets_the_window() {
        // 私は。彼は : the 。 separator splits the sentences → the two は are not paired.
        let diags =
            diagnose_with_morphology(&NoDoubledJoshi::new(), "私は。彼は", |pid, b| {
                word(b, pid, 0, 3); // 私
                joshi(b, pid, 3, 6, "係助詞"); // は
                word(b, pid, 9, 12); // 彼  (。 at 6..9)
                joshi(b, pid, 12, 15, "係助詞"); // は
            });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_op_without_a_morphology_table() {
        // In production today (no JA provider injected) the engine passes no table → rule skipped.
        assert!(diagnose(&NoDoubledJoshi::new(), "私は彼は").is_empty());
    }

    /// Push a 助詞 token with explicit `flags` (e.g. `Token::FLAG_UNKNOWN`).
    fn joshi_flagged(
        b: &mut MorphologyBuilder,
        node: NodeId,
        start: u32,
        end: u32,
        sub: &str,
        flags: u32,
    ) {
        b.push_token(
            TokenAttrs {
                node,
                surface: Span::new(start, end),
                lang: Lang::JA,
                tagset: Tagset::IPADIC,
                flags,
            },
            None,
            None,
            &[(FeatureKey::POS, "助詞"), (FeatureKey::POS_SUB_1, sub)],
        );
    }

    #[test]
    fn exception_wo_and_te_are_not_flagged() {
        // を(格助詞)×2 and て(接続助詞)×2 — idiomatic in non-strict mode.
        let wo = diagnose_with_morphology(&NoDoubledJoshi::new(), "本を物を", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "格助詞"); // を
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "格助詞"); // を
        });
        assert!(wo.is_empty(), "{wo:?}");
        let te = diagnose_with_morphology(&NoDoubledJoshi::new(), "見て言て", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "接続助詞"); // て
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "接続助詞"); // て
        });
        assert!(te.is_empty(), "{te:?}");
    }

    #[test]
    fn different_subtypes_are_not_doubled() {
        // と as 格助詞 vs 接続助詞 → different keys → not a repeat.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "犬と猫と", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "格助詞"); // と
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "接続助詞"); // と
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn flags_doubled_de() {
        // で(格助詞)×2 → flagged.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "車で家で", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "格助詞"); // で
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "格助詞"); // で
        });
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("助詞 \"で\""),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn parallel_particles_group_is_skipped() {
        // たり(並立助詞)×2 — a 並立助詞-only group is idiomatic, not flagged.
        let diags =
            diagnose_with_morphology(&NoDoubledJoshi::new(), "見たり聞たり", |p, b| {
                word(b, p, 0, 3);
                joshi(b, p, 3, 9, "並立助詞"); // たり
                word(b, p, 9, 12);
                joshi(b, p, 12, 18, "並立助詞"); // たり
            });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn ka_douka_pattern_is_not_flagged() {
        // か…どう…か (かどうか) → the か pair is skipped in non-strict mode.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "行かどうか", |p, b| {
            word(b, p, 0, 3); // 行
            joshi(b, p, 3, 6, "副助詞"); // か
            word(b, p, 6, 12); // どう
            joshi(b, p, 12, 15, "副助詞"); // か
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn three_ka_flags_only_the_non_ka_douka_pair() {
        // 見かどうか来か: (か1,か3) is かどうか (skipped); (か3,か5) has 来 between (non-contiguous,
        // not merged) and flags exactly once. The third か is separated by a word so the two later
        // か are not fused into a 連語.
        let diags =
            diagnose_with_morphology(&NoDoubledJoshi::new(), "見かどうか来か", |p, b| {
                word(b, p, 0, 3); // 見            tok 0
                joshi(b, p, 3, 6, "副助詞"); // か   tok 1
                word(b, p, 6, 12); // どう          tok 2
                joshi(b, p, 12, 15, "副助詞"); // か  tok 3
                word(b, p, 15, 18); // 来           tok 4
                joshi(b, p, 18, 21, "副助詞"); // か  tok 5
            });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (18, 21));
    }

    #[test]
    fn comma_raises_interval_to_the_default_threshold() {
        // 雨は、風は: one 、 between the two は → interval 1, not < default min 1 → passes.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "雨は、風は", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "係助詞"); // は
            word(b, p, 9, 12); // 風  (、 at 6..9)
            joshi(b, p, 12, 15, "係助詞"); // は
        });
        assert!(diags.is_empty(), "{diags:?}");
        // min_interval:2 makes interval 1 < 2 → flagged.
        let strict_gap = diagnose_with_morphology(
            &NoDoubledJoshi::from_options(&serde_json::json!({"min_interval": 2})),
            "雨は、風は",
            |p, b| {
                word(b, p, 0, 3);
                joshi(b, p, 3, 6, "係助詞");
                word(b, p, 9, 12);
                joshi(b, p, 12, 15, "係助詞");
            },
        );
        assert_eq!(strict_gap.len(), 1);
    }

    #[test]
    fn rengo_compound_doubled_is_flagged_but_distinct_keys_are_not() {
        // 東京には大阪には: に+は merge to には twice → same 連語 key → flagged.
        let dup = diagnose_with_morphology(
            &NoDoubledJoshi::new(),
            "東京には大阪には",
            |p, b| {
                word(b, p, 0, 6); // 東京
                joshi(b, p, 6, 9, "格助詞"); // に
                joshi(b, p, 9, 12, "係助詞"); // は (contiguous → には)
                word(b, p, 12, 18); // 大阪
                joshi(b, p, 18, 21, "格助詞"); // に
                joshi(b, p, 21, 24, "係助詞"); // は (→ には)
            },
        );
        assert_eq!(dup.len(), 1, "{dup:?}");
        assert!(
            dup[0].message.contains("助詞 \"には\""),
            "{}",
            dup[0].message
        );
        // 東京に大阪には: lone に vs には → different keys → not doubled.
        let distinct =
            diagnose_with_morphology(&NoDoubledJoshi::new(), "東京に大阪には", |p, b| {
                word(b, p, 0, 6); // 東京
                joshi(b, p, 6, 9, "格助詞"); // に (lone — 大阪 is a word)
                word(b, p, 9, 15); // 大阪
                joshi(b, p, 15, 18, "格助詞"); // に
                joshi(b, p, 18, 21, "係助詞"); // は (→ には)
            });
        assert!(distinct.is_empty(), "{distinct:?}");
    }

    #[test]
    fn strict_mode_flags_exceptions() {
        // strict:true disables the の/を/て exception → の×2 flags.
        let rule = NoDoubledJoshi::from_options(&serde_json::json!({"strict": true}));
        let diags = diagnose_with_morphology(&rule, "本のペンの", |p, b| {
            word(b, p, 0, 3); // 本
            joshi(b, p, 3, 6, "連体化"); // の
            word(b, p, 6, 12); // ペン
            joshi(b, p, 12, 15, "連体化"); // の
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn allow_suppresses_a_surface() {
        // allow:["も"] drops も before any other logic.
        let rule = NoDoubledJoshi::from_options(&serde_json::json!({"allow": ["も"]}));
        let diags = diagnose_with_morphology(&rule, "犬も猫も", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "係助詞"); // も
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "係助詞"); // も
        });
        assert!(diags.is_empty(), "{diags:?}");
        // Without allow, も×2 flags.
        let flagged = diagnose_with_morphology(&NoDoubledJoshi::new(), "犬も猫も", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "係助詞");
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "係助詞");
        });
        assert_eq!(flagged.len(), 1);
    }

    #[test]
    fn detection_is_coupled_to_the_ipadic_pos_literal() {
        // A token whose major POS is NOT the bare "助詞" literal (e.g. a UniDic-style fused tag) is
        // not treated as a particle → the rule no-ops (a false negative, never a false positive).
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "私は彼は", |p, b| {
            word(b, p, 0, 3);
            b.push_token(
                TokenAttrs {
                    node: p,
                    surface: Span::new(3, 6),
                    lang: Lang::JA,
                    tagset: Tagset::UNIDIC,
                    flags: 0,
                },
                None,
                None,
                &[(FeatureKey::POS, "助詞-係助詞")],
            );
            word(b, p, 6, 9);
            b.push_token(
                TokenAttrs {
                    node: p,
                    surface: Span::new(9, 12),
                    lang: Lang::JA,
                    tagset: Tagset::UNIDIC,
                    flags: 0,
                },
                None,
                None,
                &[(FeatureKey::POS, "助詞-係助詞")],
            );
        });
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn oov_tokens_are_not_trusted() {
        use tzlint_ast::morphology::Token;
        // Both は flagged FLAG_UNKNOWN → skipped (OOV guess not trusted) → 0.
        let oov = diagnose_with_morphology(&NoDoubledJoshi::new(), "私は彼は", |p, b| {
            word(b, p, 0, 3);
            joshi_flagged(b, p, 3, 6, "係助詞", Token::FLAG_UNKNOWN);
            word(b, p, 6, 9);
            joshi_flagged(b, p, 9, 12, "係助詞", Token::FLAG_UNKNOWN);
        });
        assert!(oov.is_empty(), "{oov:?}");
        // The same pair without the flag IS flagged — proves the skip is causal.
        let known = diagnose_with_morphology(&NoDoubledJoshi::new(), "私は彼は", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "係助詞");
            word(b, p, 6, 9);
            joshi(b, p, 9, 12, "係助詞");
        });
        assert_eq!(known.len(), 1);
    }

    #[test]
    fn defensive_sort_handles_unordered_tokens() {
        // Push the two は OUT of source order; the rule sorts by surface.start, so it still flags
        // once at the source-second は (9..12) — proving the in-rule sort, not the producer order.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "私は彼は", |p, b| {
            joshi(b, p, 9, 12, "係助詞"); // は (source-second) pushed FIRST
            word(b, p, 6, 9);
            joshi(b, p, 3, 6, "係助詞"); // は (source-first) pushed LAST
            word(b, p, 0, 3);
        });
        assert_eq!(diags.len(), 1);
        assert_eq!((diags[0].span.start, diags[0].span.end), (9, 12));
    }

    #[test]
    fn build_rule_routes_options() {
        use crate::build_rule;
        // The options reach the constructed rule: strict flags の×2, default does not.
        let strict = build_rule(
            "no-doubled-joshi",
            &serde_json::json!({"strict": true}),
            None,
        )
        .unwrap();
        let diags = diagnose_with_morphology(strict.as_ref(), "本のペンの", |p, b| {
            word(b, p, 0, 3);
            joshi(b, p, 3, 6, "連体化");
            word(b, p, 6, 12);
            joshi(b, p, 12, 15, "連体化");
        });
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn message_for_a_leading_particle_omits_the_prev_word() {
        // は犬は: the first は is the very first token (no preceding word) → its body line is the
        // bare `- "は"` form. Both は share a key and flag once.
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "は犬は", |p, b| {
            joshi(b, p, 0, 3, "係助詞"); // は (token 0 — no prev word)
            word(b, p, 3, 6); // 犬
            joshi(b, p, 6, 9, "係助詞"); // は
        });
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("- \"は\""),
            "{}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("- 犬\"は\""),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn custom_separator_splits_sentences() {
        // A custom separator `♪` resets the window; without it the two は are one sentence → flagged.
        let src = "雨は♪風は";
        let build = |p: NodeId, b: &mut MorphologyBuilder| {
            word(b, p, 0, 3); // 雨
            joshi(b, p, 3, 6, "係助詞"); // は
            word(b, p, 9, 12); // 風  (♪ = 3 bytes at 6..9)
            joshi(b, p, 12, 15, "係助詞"); // は
        };
        let custom =
            NoDoubledJoshi::from_options(&serde_json::json!({"separator_characters": ["♪"]}));
        assert!(diagnose_with_morphology(&custom, src, build).is_empty());
        assert_eq!(
            diagnose_with_morphology(&NoDoubledJoshi::new(), src, build).len(),
            1
        );
    }

    #[test]
    fn custom_comma_raises_the_interval() {
        // A custom comma `・` raises the gap to 1 (not < default min 1) → suppressed; default does not.
        let src = "雨は・風は";
        let build = |p: NodeId, b: &mut MorphologyBuilder| {
            word(b, p, 0, 3); // 雨
            joshi(b, p, 3, 6, "係助詞"); // は
            word(b, p, 9, 12); // 風  (・ = 3 bytes at 6..9)
            joshi(b, p, 12, 15, "係助詞"); // は
        };
        let custom = NoDoubledJoshi::from_options(&serde_json::json!({"comma_characters": ["・"]}));
        assert!(diagnose_with_morphology(&custom, src, build).is_empty());
        assert_eq!(
            diagnose_with_morphology(&NoDoubledJoshi::new(), src, build).len(),
            1
        );
    }

    #[test]
    fn a_bracket_raises_the_interval() {
        // A bracket between two は raises the gap to 1 (not < default min 1) → suppressed; deleting
        // the BRACKETS arm of `interval` would flag it instead (mirror of the comma test).
        let src = "雨は(風は"; // half-width '(' = 1 byte at offset 6
        let build = |p: NodeId, b: &mut MorphologyBuilder| {
            word(b, p, 0, 3); // 雨
            joshi(b, p, 3, 6, "係助詞"); // は
            word(b, p, 7, 10); // 風  ('(' at 6..7)
            joshi(b, p, 10, 13, "係助詞"); // は
        };
        assert!(diagnose_with_morphology(&NoDoubledJoshi::new(), src, build).is_empty());
        // min_interval 2 makes the weight-1 bracket gap flag.
        let strict = NoDoubledJoshi::from_options(&serde_json::json!({"min_interval": 2}));
        let diags = diagnose_with_morphology(&strict, src, build);
        assert_eq!(diags.len(), 1);
        assert_eq!((diags[0].span.start, diags[0].span.end), (10, 13));
    }

    #[test]
    fn flags_a_subtype_less_particle() {
        // A 助詞 with POS but no POS_SUB_1 → key "さ:助詞" (the no-subtype key arm); two flag once.
        fn joshi_no_subtype(b: &mut MorphologyBuilder, node: NodeId, start: u32, end: u32) {
            b.push_token(
                TokenAttrs {
                    node,
                    surface: Span::new(start, end),
                    lang: Lang::JA,
                    tagset: Tagset::IPADIC,
                    flags: 0,
                },
                None,
                None,
                &[(FeatureKey::POS, "助詞")],
            );
        }
        let diags = diagnose_with_morphology(&NoDoubledJoshi::new(), "私さ彼さ", |p, b| {
            word(b, p, 0, 3);
            joshi_no_subtype(b, p, 3, 6); // さ
            word(b, p, 6, 9);
            joshi_no_subtype(b, p, 9, 12); // さ
        });
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].span.start, diags[0].span.end), (9, 12));
    }

    #[test]
    fn each_sentence_flags_its_own_repeat() {
        // Positive multi-sentence: は×2 in sentence 0 AND も×2 in sentence 1 → a flag in EACH.
        let diags = diagnose_with_morphology(
            &NoDoubledJoshi::new(),
            "猫は犬は。魚も鳥も",
            |p, b| {
                word(b, p, 0, 3); // 猫
                joshi(b, p, 3, 6, "係助詞"); // は  sentence 0
                word(b, p, 6, 9); // 犬
                joshi(b, p, 9, 12, "係助詞"); // は  sentence 0
                word(b, p, 15, 18); // 魚  (。 at 12..15)
                joshi(b, p, 18, 21, "係助詞"); // も  sentence 1
                word(b, p, 21, 24); // 鳥
                joshi(b, p, 24, 27, "係助詞"); // も  sentence 1
            },
        );
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().any(|d| d.span.end <= 12), "{diags:?}"); // sentence 0
        assert!(diags.iter().any(|d| d.span.start >= 15), "{diags:?}"); // sentence 1
    }

    #[test]
    fn same_particle_across_distant_sentences_is_not_paired() {
        // は once in sentence 1 and once in sentence 2 → never grouped (sentence_idx 1 vs 2). This
        // pins that the per-particle sentence index is the sole sentence boundary now the redundant
        // interval separator-guard is gone: a `min(idx, 1)` clamp would wrongly fuse them and flag.
        let diags =
            diagnose_with_morphology(&NoDoubledJoshi::new(), "口。猫は。犬は", |p, b| {
                word(b, p, 0, 3); // 口  (。 at 3..6)
                word(b, p, 6, 9); // 猫
                joshi(b, p, 9, 12, "係助詞"); // は  sentence 1
                word(b, p, 15, 18); // 犬  (。 at 12..15)
                joshi(b, p, 18, 21, "係助詞"); // は  sentence 2
            });
        assert!(diags.is_empty(), "{diags:?}");
    }
}
