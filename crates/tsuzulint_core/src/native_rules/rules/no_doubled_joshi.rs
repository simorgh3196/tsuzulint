//! Native port of `no-doubled-joshi`.
//!
//! Flags repeated Japanese particles (助詞) inside a single sentence. Mirrors
//! the semantics of `textlint-rule-no-doubled-joshi` and the existing WASM
//! `rules/no-doubled-joshi` plugin. Needs morphological tokens, which the
//! linter populates because the rule declares `needs_morphology()`.

use serde_json::Value;
use std::collections::HashMap;
use tsuzulint_ast::Span;
use tsuzulint_plugin::{Diagnostic, Severity};
use tsuzulint_text::Token;

use crate::native_rules::{Rule, RuleContext};

const RULE_ID: &str = "no-doubled-joshi";
const DEFAULT_SEPARATORS: &[char] = &['.', '．', '。', '?', '!', '？', '！'];
const DEFAULT_COMMAS: &[char] = &['、', '，'];

struct Config {
    min_interval: usize,
    strict: bool,
    allow: Vec<String>,
    separators: Vec<char>,
    commas: Vec<char>,
}

impl Config {
    fn from_options(options: &Value) -> Self {
        let mut cfg = Self {
            min_interval: 1,
            strict: false,
            allow: Vec::new(),
            separators: DEFAULT_SEPARATORS.to_vec(),
            commas: DEFAULT_COMMAS.to_vec(),
        };
        let Value::Object(map) = options else {
            return cfg;
        };
        if let Some(Value::Number(n)) = map.get("min_interval")
            && let Some(v) = n.as_u64()
        {
            cfg.min_interval = v as usize;
        }
        if let Some(Value::Bool(b)) = map.get("strict") {
            cfg.strict = *b;
        }
        if let Some(Value::Array(arr)) = map.get("allow") {
            cfg.allow = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
        }
        if let Some(Value::Array(arr)) = map.get("separator_characters") {
            cfg.separators = arr
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| s.chars().next()))
                .collect();
        }
        if let Some(Value::Array(arr)) = map.get("comma_characters") {
            cfg.commas = arr
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| s.chars().next()))
                .collect();
        }
        cfg
    }
}

pub struct NoDoubledJoshi;
pub static RULE: NoDoubledJoshi = NoDoubledJoshi;

impl Rule for NoDoubledJoshi {
    fn name(&self) -> &'static str {
        RULE_ID
    }

    fn description(&self) -> &'static str {
        "Detect the same Japanese particle repeating within a single sentence."
    }

    fn needs_morphology(&self) -> bool {
        true
    }

    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let config = Config::from_options(ctx.options);
        let tokens = ctx.tokens;
        if tokens.is_empty() {
            // Linter didn't provide tokens (e.g. tokenizer disabled for non-JP);
            // nothing for us to do.
            return Vec::new();
        }
        let particles = extract_particles(tokens, &config);
        let particles = concat_adjacent(particles);
        detect_doubles(&particles, ctx.source, &config)
    }
}

#[derive(Debug, Clone)]
struct Particle {
    surface: String,
    key: String,
    byte_start: usize,
    byte_end: usize,
    /// Reserved for future pair-level heuristics (e.g. recovering the exact
    /// tokens surrounding a possible match). Not used today, but removing it
    /// would complicate re-adding such heuristics.
    #[allow(dead_code)]
    token_index: usize,
    subtype: Option<String>,
    prev_word: String,
}

fn is_particle(tok: &Token) -> bool {
    tok.pos.first().map(String::as_str) == Some("助詞")
}

fn particle_subtype(tok: &Token) -> Option<String> {
    tok.pos.get(1).cloned()
}

/// `の`(連体化), `を`(格助詞), `て`(接続助詞) are commonly doubled in clean
/// prose — textlint's no-doubled-joshi exempts them unless `strict` is set.
fn is_exception_particle(tok: &Token) -> bool {
    let subtype = particle_subtype(tok);
    matches!(
        (tok.surface.as_str(), subtype.as_deref()),
        ("の", Some("連体化")) | ("を", Some("格助詞")) | ("て", Some("接続助詞"))
    )
}

fn particle_key(tok: &Token) -> String {
    let pos1 = tok.pos.first().cloned().unwrap_or_default();
    match particle_subtype(tok) {
        Some(sub) if !sub.is_empty() => format!("{}:{}.{}", tok.surface, pos1, sub),
        _ => format!("{}:{}", tok.surface, pos1),
    }
}

fn extract_particles(tokens: &[Token], config: &Config) -> Vec<Particle> {
    let mut out = Vec::new();
    for (idx, tok) in tokens.iter().enumerate() {
        if !is_particle(tok) {
            continue;
        }
        if config.allow.contains(&tok.surface) {
            continue;
        }
        if !config.strict && is_exception_particle(tok) {
            continue;
        }
        let prev_word = if idx == 0 {
            String::new()
        } else {
            tokens[idx - 1].surface.clone()
        };
        out.push(Particle {
            surface: tok.surface.clone(),
            key: particle_key(tok),
            byte_start: tok.span.start,
            byte_end: tok.span.end,
            token_index: idx,
            subtype: particle_subtype(tok),
            prev_word,
        });
    }
    out
}

/// Merge adjacent particles into a compound (連語) so "に" + "は" becomes
/// "には" and is compared against other "には" occurrences.
fn concat_adjacent(particles: Vec<Particle>) -> Vec<Particle> {
    let mut out: Vec<Particle> = Vec::with_capacity(particles.len());
    for p in particles {
        if let Some(curr) = out.last_mut()
            && p.byte_start == curr.byte_end
        {
            curr.surface.push_str(&p.surface);
            curr.key = format!("{}:連語", curr.surface);
            curr.byte_end = p.byte_end;
            curr.subtype = Some("連語".to_string());
            continue;
        }
        out.push(p);
    }
    out
}

fn calculate_interval(prev: &Particle, curr: &Particle, source: &str, config: &Config) -> usize {
    if curr.byte_start <= prev.byte_end {
        return 0;
    }
    let between = &source[prev.byte_end..curr.byte_start];
    let mut interval = 0usize;
    for c in between.chars() {
        if config.separators.contains(&c) {
            return usize::MAX;
        }
        if config.commas.contains(&c) {
            interval += 1;
        }
        if matches!(c, '(' | ')' | '（' | '）' | '「' | '」') {
            interval += 1;
        }
    }
    interval
}

fn is_ka_douka_pair(p1: &Particle, p2: &Particle, tokens_between: &str) -> bool {
    p1.surface == "か" && p2.surface == "か" && tokens_between.contains("どう")
}

fn detect_doubles(particles: &[Particle], source: &str, config: &Config) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut by_key: HashMap<&str, Vec<&Particle>> = HashMap::new();
    for p in particles {
        by_key.entry(p.key.as_str()).or_default().push(p);
    }
    for (_key, matches) in by_key {
        if matches.len() < 2 {
            continue;
        }
        if !config.strict
            && matches
                .iter()
                .all(|p| p.subtype.as_deref() == Some("並立助詞"))
        {
            continue;
        }
        for i in 1..matches.len() {
            let prev = matches[i - 1];
            let curr = matches[i];
            let between = &source[prev.byte_end..curr.byte_start];
            if !config.strict && is_ka_douka_pair(prev, curr, between) {
                continue;
            }
            let interval = calculate_interval(prev, curr, source, config);
            if interval < config.min_interval {
                let message = build_message(&curr.surface, prev, curr);
                out.push(
                    Diagnostic::new(
                        RULE_ID,
                        message,
                        Span::new(curr.byte_start as u32, curr.byte_end as u32),
                    )
                    .with_severity(Severity::Warning),
                );
            }
        }
    }
    // Stable order: deterministic diagnostic output.
    out.sort_unstable();
    out.dedup();
    out
}

fn build_message(name: &str, prev: &Particle, curr: &Particle) -> String {
    let mut msg = format!(
        "一文に二回以上利用されている助詞 \"{}\" がみつかりました。\n\n文中の助詞が連続しているため、読みにくくなります。\n\n",
        name
    );
    for p in [prev, curr] {
        if p.prev_word.is_empty() {
            msg.push_str(&format!("- \"{}\"\n", p.surface));
        } else {
            msg.push_str(&format!("- {}\"{}\"\n", p.prev_word, p.surface));
        }
    }
    let _ = curr;
    msg.push_str(
        "\n同じ助詞を連続して利用しない、文の中で順番を入れ替える、文を分割するなどを検討してください。\n",
    );
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(surface: &str, pos: Vec<&str>, start: usize, end: usize) -> Token {
        Token {
            surface: surface.to_string(),
            pos: pos.iter().map(|s| s.to_string()).collect(),
            detail: Vec::new(),
            span: start..end,
        }
    }

    #[test]
    fn flags_doubled_ha() {
        let source = "私は彼は好きだ";
        let tokens = vec![
            make_token("私", vec!["名詞"], 0, 3),
            make_token("は", vec!["助詞", "係助詞"], 3, 6),
            make_token("彼", vec!["名詞"], 6, 9),
            make_token("は", vec!["助詞", "係助詞"], 9, 12),
            make_token("好き", vec!["名詞"], 12, 18),
        ];
        let config = Config::from_options(&Value::Null);
        let particles = extract_particles(&tokens, &config);
        let diags = detect_doubles(&particles, source, &config);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("\"は\""));
    }

    #[test]
    fn skips_no_exception() {
        let tokens = vec![
            make_token("既存", vec!["名詞"], 0, 6),
            make_token("の", vec!["助詞", "連体化"], 6, 9),
            make_token("コード", vec!["名詞"], 9, 15),
            make_token("の", vec!["助詞", "連体化"], 15, 18),
            make_token("利用", vec!["名詞"], 18, 24),
        ];
        let config = Config::from_options(&Value::Null);
        let particles = extract_particles(&tokens, &config);
        assert!(particles.is_empty(), "の(連体化) should be exempt");
    }

    #[test]
    fn strict_mode_catches_no() {
        let source = "既存のコードの利用";
        let tokens = vec![
            make_token("既存", vec!["名詞"], 0, 6),
            make_token("の", vec!["助詞", "連体化"], 6, 9),
            make_token("コード", vec!["名詞"], 9, 15),
            make_token("の", vec!["助詞", "連体化"], 15, 18),
            make_token("利用", vec!["名詞"], 18, 24),
        ];
        let options = serde_json::json!({ "strict": true });
        let config = Config::from_options(&options);
        let particles = extract_particles(&tokens, &config);
        let diags = detect_doubles(&particles, source, &config);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn concat_ni_ha() {
        let particles = vec![
            Particle {
                surface: "に".into(),
                key: "に:助詞.格助詞".into(),
                byte_start: 0,
                byte_end: 3,
                token_index: 0,
                subtype: Some("格助詞".into()),
                prev_word: "".into(),
            },
            Particle {
                surface: "は".into(),
                key: "は:助詞.係助詞".into(),
                byte_start: 3,
                byte_end: 6,
                token_index: 1,
                subtype: Some("係助詞".into()),
                prev_word: "".into(),
            },
        ];
        let merged = concat_adjacent(particles);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].surface, "には");
    }

    #[test]
    fn allow_suppresses() {
        let tokens = vec![
            make_token("太字", vec!["名詞"], 0, 6),
            make_token("も", vec!["助詞", "係助詞"], 6, 9),
            make_token("強調", vec!["名詞"], 9, 15),
            make_token("も", vec!["助詞", "係助詞"], 15, 18),
        ];
        let options = serde_json::json!({ "allow": ["も"] });
        let config = Config::from_options(&options);
        let particles = extract_particles(&tokens, &config);
        assert!(particles.is_empty());
    }

    #[test]
    fn interval_below_min_is_flagged() {
        // Two は with 0 commas between them. Default min_interval=1 flags this.
        let source = "私は彼は好きだ";
        let tokens = vec![
            make_token("私", vec!["名詞"], 0, 3),
            make_token("は", vec!["助詞", "係助詞"], 3, 6),
            make_token("彼", vec!["名詞"], 6, 9),
            make_token("は", vec!["助詞", "係助詞"], 9, 12),
            make_token("好き", vec!["名詞"], 12, 18),
        ];
        let config = Config::from_options(&Value::Null);
        let particles = extract_particles(&tokens, &config);
        let diags = detect_doubles(&particles, source, &config);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn interval_at_min_passes() {
        // Two は with one 、 between them. interval=1 == min_interval=1 passes.
        let source = "私は、彼は好きだ";
        let tokens = vec![
            make_token("私", vec!["名詞"], 0, 3),
            make_token("は", vec!["助詞", "係助詞"], 3, 6),
            make_token("、", vec!["記号", "読点"], 6, 9),
            make_token("彼", vec!["名詞"], 9, 12),
            make_token("は", vec!["助詞", "係助詞"], 12, 15),
            make_token("好き", vec!["名詞"], 15, 21),
        ];
        let config = Config::from_options(&Value::Null);
        let particles = extract_particles(&tokens, &config);
        let diags = detect_doubles(&particles, source, &config);
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn interval_one_below_custom_min_is_flagged() {
        // min_interval=2, but text has one 、 between the は particles.
        // interval=1 < min_interval=2 → flagged at the boundary.
        let source = "私は、彼は好きだ";
        let tokens = vec![
            make_token("私", vec!["名詞"], 0, 3),
            make_token("は", vec!["助詞", "係助詞"], 3, 6),
            make_token("、", vec!["記号", "読点"], 6, 9),
            make_token("彼", vec!["名詞"], 9, 12),
            make_token("は", vec!["助詞", "係助詞"], 12, 15),
            make_token("好き", vec!["名詞"], 15, 21),
        ];
        let options = serde_json::json!({ "min_interval": 2 });
        let config = Config::from_options(&options);
        let particles = extract_particles(&tokens, &config);
        let diags = detect_doubles(&particles, source, &config);
        assert_eq!(diags.len(), 1);
    }
}
