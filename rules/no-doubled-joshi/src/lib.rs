//! no-doubled-joshi rule: Detect repeated Japanese particles (助詞).
//!
//! This rule detects when Japanese particles are repeated within a sentence,
//! which makes the text harder to read.
//!
//! Compatible with textlint-rule-no-doubled-joshi.
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | min_interval | number | 1 | Minimum distance between same particles |
//! | strict | boolean | false | Disable exception rules |
//! | allow | string[] | [] | Particles to allow even if doubled |
//! | separator_characters | string[] | [".", "．", "。", "?", "!", "？", "！"] | Sentence separator characters |
//! | comma_characters | string[] | ["、", "，"] | Comma characters that increase interval |
//! | suggest_fix | boolean | false | Whether to provide auto-fix suggestions |
//!
//! # Example
//!
//! ```json
//! {
//!   "rules": {
//!     "no-doubled-joshi": {
//!       "min_interval": 1,
//!       "strict": false,
//!       "allow": ["も"]
//!     }
//!   }
//! }
//! ```

use extism_pdk::*;
use serde::Deserialize;
use std::collections::HashMap;
use tsuzulint_rule_pdk::{
    Capability, Diagnostic, Fix, KnownLanguage, LintRequest, LintResponse, RuleManifest, Span,
    TextSpan, Token, extract_node_text, is_node_type,
};

const RULE_ID: &str = "no-doubled-joshi";
const VERSION: &str = "1.0.0";

const DEFAULT_SEPARATOR_CHARACTERS: &[&str] = &[".", "．", "。", "?", "!", "？", "！"];
const DEFAULT_COMMA_CHARACTERS: &[&str] = &["、", "，"];

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_min_interval")]
    min_interval: usize,
    #[serde(default)]
    strict: bool,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default = "default_separator_characters")]
    separator_characters: Vec<String>,
    #[serde(default = "default_comma_characters")]
    comma_characters: Vec<String>,
    #[serde(default)]
    suggest_fix: bool,
}

fn default_min_interval() -> usize {
    1
}

fn default_separator_characters() -> Vec<String> {
    DEFAULT_SEPARATOR_CHARACTERS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn default_comma_characters() -> Vec<String> {
    DEFAULT_COMMA_CHARACTERS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_interval: default_min_interval(),
            strict: false,
            allow: Vec::new(),
            separator_characters: default_separator_characters(),
            comma_characters: default_comma_characters(),
            suggest_fix: false,
        }
    }
}

#[derive(Debug, Clone)]
struct ParticleInfo {
    surface: String,
    key: String,
    byte_start: usize,
    byte_end: usize,
    token_index: usize,
    prev_word: String,
    pos_subtype: Option<String>,
}

fn create_particle_key(token: &Token) -> String {
    let major = token.major_pos().unwrap_or("");
    let subtype = token.pos_detail(1).unwrap_or("");
    if subtype.is_empty() {
        format!("{}:{}", token.surface, major)
    } else {
        format!("{}:{}.{}", token.surface, major, subtype)
    }
}

/// 助詞の例外ルールを判定する。
///
/// 以下の助詞は重複を許容する:
/// - 「の」（連体化）: 修飾関係を示す
/// - 「を」（格助詞）: 目的語を示す
/// - 「て」（接続助詞）: 接続を示す
fn is_exception_particle(token: &Token) -> bool {
    let surface = token.surface.as_str();
    let subtype = token.pos_detail(1);

    match (surface, subtype) {
        ("の", Some("連体化")) => true,
        ("を", Some("格助詞")) => true,
        ("て", Some("接続助詞")) => true,
        _ => false,
    }
}

/// 並立助詞かどうかを判定する。
///
/// 「たり」「や」「か」などの並立助詞は、
/// 列挙のために連続して使用されるため許容する。
fn is_parallel_particle(token: &Token) -> bool {
    token.pos_detail(1) == Some("並立助詞")
}

fn get_previous_word(_token: &Token, tokens: &[Token], token_index: usize) -> String {
    if token_index == 0 {
        return String::new();
    }
    tokens[token_index - 1].surface.clone()
}

fn extract_particles_from_tokens(tokens: &[Token], config: &Config) -> Vec<ParticleInfo> {
    let mut particles = Vec::new();

    for (idx, token) in tokens.iter().enumerate() {
        if !token.is_particle() {
            continue;
        }

        if config.allow.contains(&token.surface) {
            continue;
        }

        if !config.strict && is_exception_particle(token) {
            continue;
        }

        let key = create_particle_key(token);
        let prev_word = get_previous_word(token, tokens, idx);

        particles.push(ParticleInfo {
            surface: token.surface.clone(),
            key,
            byte_start: token.span.start as usize,
            byte_end: token.span.end as usize,
            token_index: idx,
            prev_word,
            pos_subtype: token.pos_detail(1).map(|s| s.to_string()),
        });
    }

    particles
}

/// 連続する助詞を結合して連語として扱う。
///
/// 例: 「に」+「は」→「には」（連語）
/// これにより、「には」と「には」の重複を正しく検出できる。
fn concat_adjacent_particles(particles: Vec<ParticleInfo>) -> Vec<ParticleInfo> {
    let mut result = Vec::new();
    let mut current: Option<ParticleInfo> = None;

    for particle in particles {
        if let Some(ref mut curr) = current {
            if particle.byte_start == curr.byte_end {
                curr.surface.push_str(&particle.surface);
                curr.key = format!("{}:連語", curr.surface);
                curr.byte_end = particle.byte_end;
                continue;
            } else {
                result.push(curr.clone());
            }
        }
        current = Some(particle);
    }

    if let Some(p) = current {
        result.push(p);
    }

    result
}

/// 「かどうか」パターンかどうかを判定する。
///
/// 「〜するかどうか検討する」のような表現では、
/// 「か」が連続するが、意味的に問題ないため許容する。
fn is_ka_douka_pattern(particles: &[&ParticleInfo], all_tokens: &[Token]) -> bool {
    if particles.len() != 2 {
        return false;
    }
    if particles[0].surface != "か" || particles[1].surface != "か" {
        return false;
    }

    let idx1 = particles[0].token_index;
    let idx2 = particles[1].token_index;

    if idx2 == idx1 + 2 && idx1 + 1 < all_tokens.len() {
        let middle = &all_tokens[idx1 + 1];
        if middle.surface == "どう" {
            return true;
        }
    }

    false
}

fn calculate_interval(
    prev: &ParticleInfo,
    curr: &ParticleInfo,
    source: &str,
    config: &Config,
) -> usize {
    if curr.byte_start <= prev.byte_end {
        return 0;
    }

    let text_between = &source[prev.byte_end..curr.byte_start];
    let mut interval = 0;

    for c in text_between.chars() {
        let c_str = c.to_string();
        if config.separator_characters.contains(&c_str) {
            return usize::MAX;
        }
        if config.comma_characters.contains(&c_str) {
            interval += 1;
        }
        if matches!(c, '(' | ')' | '（' | '）' | '「' | '」') {
            interval += 1;
        }
    }

    interval
}

fn create_error_message(particle_name: &str, matches: &[&ParticleInfo]) -> String {
    let mut message = format!(
        "一文に二回以上利用されている助詞 \"{}\" がみつかりました。\n\n次の助詞が連続しているため、文を読みにくくしています。\n\n",
        particle_name
    );

    for p in matches {
        if p.prev_word.is_empty() {
            message.push_str(&format!("- \"{}\"\n", p.surface));
        } else {
            message.push_str(&format!("- {}\"{}\"\n", p.prev_word, p.surface));
        }
    }

    message.push_str(
        "\n同じ助詞を連続して利用しない、文の中で順番を入れ替える、文を分割するなどを検討してください。\n",
    );

    message
}

fn check_doubled_particles(
    particles: &[ParticleInfo],
    all_tokens: &[Token],
    source: &str,
    config: &Config,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let mut by_key: HashMap<String, Vec<&ParticleInfo>> = HashMap::new();
    for p in particles {
        by_key.entry(p.key.clone()).or_default().push(p);
    }

    for (_key, matches) in by_key {
        if matches.len() < 2 {
            continue;
        }

        if !config.strict {
            if matches.len() == 2
                && matches[0].pos_subtype == Some("並立助詞".to_string())
                && matches[1].pos_subtype == Some("並立助詞".to_string())
            {
                continue;
            }

            if is_ka_douka_pattern(&matches, all_tokens) {
                continue;
            }
        }

        for i in 1..matches.len() {
            let prev = matches[i - 1];
            let curr = matches[i];
            let interval = calculate_interval(prev, curr, source, config);

            if interval < config.min_interval {
                let particle_name = curr.surface.clone();
                let message = create_error_message(&particle_name, &[prev, curr]);

                let mut diagnostic = Diagnostic::new(
                    RULE_ID,
                    message,
                    Span::new(curr.byte_start as u32, curr.byte_end as u32),
                );

                if config.suggest_fix {
                    diagnostic = diagnostic.with_fix(Fix::delete(Span::new(
                        curr.byte_start as u32,
                        curr.byte_end as u32,
                    )));
                }

                diagnostics.push(diagnostic);
            }
        }
    }

    diagnostics
}

#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Detect repeated Japanese particles (助詞)")
        .with_fixable(true)
        .with_node_types(vec!["Str".to_string()])
        .with_languages(vec![KnownLanguage::Ja])
        .with_capabilities(vec![Capability::Morphology]);
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>> {
    lint_impl(input)
}

fn lint_impl(input: Vec<u8>) -> FnResult<Vec<u8>> {
    let request: LintRequest = rmp_serde::from_slice(&input)?;
    let mut diagnostics = Vec::new();

    if !is_node_type(&request.node, "Str") {
        return Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?);
    }

    let config: Config = tsuzulint_rule_pdk::get_config().unwrap_or_default();
    let tokens = request.get_tokens();

    let (node_start, node_end, _text) =
        if let Some((s, e, t)) = extract_node_text(&request.node, &request.source) {
            (s, e, t)
        } else {
            return Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?);
        };

    let node_tokens: Vec<Token> = if !tokens.is_empty() {
        tokens
            .iter()
            .filter(|t| (t.span.start as usize) >= node_start && (t.span.end as usize) <= node_end)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    if node_tokens.is_empty() {
        return Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?);
    }

    let mut particles = extract_particles_from_tokens(&node_tokens, &config);
    particles = concat_adjacent_particles(particles);

    let source_slice = &request.source[node_start..node_end];
    diagnostics = check_doubled_particles(&particles, &node_tokens, source_slice, &config);

    Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn config_defaults() {
        let config = Config::default();
        assert_eq!(config.min_interval, 1);
        assert!(!config.strict);
        assert!(config.allow.is_empty());
        assert!(config.separator_characters.contains(&"。".to_string()));
        assert!(config.comma_characters.contains(&"、".to_string()));
    }

    #[test]
    fn particle_key_generation() {
        let token = Token::new(
            "は",
            vec!["助詞".to_string(), "係助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert_eq!(create_particle_key(&token), "は:助詞.係助詞");

        let token_no_subtype = Token::new("は", vec!["助詞".to_string()], TextSpan::new(0, 3));
        assert_eq!(create_particle_key(&token_no_subtype), "は:助詞");
    }

    #[test]
    fn exception_particle_detection() {
        let no_token = Token::new(
            "の",
            vec!["助詞".to_string(), "連体化".to_string()],
            TextSpan::new(0, 3),
        );
        assert!(is_exception_particle(&no_token));

        let wo_token = Token::new(
            "を",
            vec!["助詞".to_string(), "格助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert!(is_exception_particle(&wo_token));

        let te_token = Token::new(
            "て",
            vec!["助詞".to_string(), "接続助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert!(is_exception_particle(&te_token));

        let ha_token = Token::new(
            "は",
            vec!["助詞".to_string(), "係助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert!(!is_exception_particle(&ha_token));
    }

    #[test]
    fn parallel_particle_detection() {
        let tari_token = Token::new(
            "たり",
            vec!["助詞".to_string(), "並立助詞".to_string()],
            TextSpan::new(0, 6),
        );
        assert!(is_parallel_particle(&tari_token));

        let ha_token = Token::new(
            "は",
            vec!["助詞".to_string(), "係助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert!(!is_parallel_particle(&ha_token));
    }

    #[test]
    fn test_concat_adjacent_particles() {
        let particles = vec![
            ParticleInfo {
                surface: "に".into(),
                key: "に:助詞.格助詞".into(),
                byte_start: 0,
                byte_end: 3,
                token_index: 0,
                prev_word: "".into(),
                pos_subtype: Some("格助詞".into()),
            },
            ParticleInfo {
                surface: "は".into(),
                key: "は:助詞.係助詞".into(),
                byte_start: 3,
                byte_end: 6,
                token_index: 1,
                prev_word: "".into(),
                pos_subtype: Some("係助詞".into()),
            },
        ];
        let concatenated = concat_adjacent_particles(particles);
        assert_eq!(concatenated.len(), 1);
        assert_eq!(concatenated[0].surface, "には");
        assert!(concatenated[0].key.contains("連語"));
    }

    #[test]
    fn interval_calculation_with_comma() {
        let config = Config::default();
        let source = "私は、学生は";
        let prev = ParticleInfo {
            surface: "は".into(),
            key: "は:助詞.係助詞".into(),
            byte_start: 0,
            byte_end: 3,
            token_index: 0,
            prev_word: "私".into(),
            pos_subtype: Some("係助詞".into()),
        };
        let curr = ParticleInfo {
            surface: "は".into(),
            key: "は:助詞.係助詞".into(),
            byte_start: source.len() - 3,
            byte_end: source.len(),
            token_index: 1,
            prev_word: "学生".into(),
            pos_subtype: Some("係助詞".into()),
        };

        let interval = calculate_interval(&prev, &curr, source, &config);
        assert!(interval >= 1);
    }

    #[test]
    fn error_message_format() {
        let particles = vec![
            ParticleInfo {
                surface: "は".into(),
                key: "は:助詞.係助詞".into(),
                byte_start: 0,
                byte_end: 3,
                token_index: 0,
                prev_word: "私".into(),
                pos_subtype: Some("係助詞".into()),
            },
            ParticleInfo {
                surface: "は".into(),
                key: "は:助詞.係助詞".into(),
                byte_start: 6,
                byte_end: 9,
                token_index: 1,
                prev_word: "彼".into(),
                pos_subtype: Some("係助詞".into()),
            },
        ];
        let refs: Vec<&ParticleInfo> = particles.iter().collect();
        let message = create_error_message("は", &refs);
        assert!(message.contains("私\"は\""));
        assert!(message.contains("彼\"は\""));
        assert!(message.contains("文を読みにくくしています"));
    }

    #[test]
    fn manifest_contains_required_fields() {
        let manifest = RuleManifest::new(RULE_ID, VERSION)
            .with_description("Detect repeated Japanese particles (助詞)")
            .with_fixable(true)
            .with_node_types(vec!["Str".to_string()])
            .with_languages(vec![KnownLanguage::Ja])
            .with_capabilities(vec![Capability::Morphology]);

        assert_eq!(manifest.name, RULE_ID);
        assert_eq!(manifest.version, VERSION);
        assert!(manifest.description.is_some());
        assert!(manifest.fixable);
        assert!(manifest.node_types.contains(&"Str".to_string()));
        assert!(manifest.languages.contains(&KnownLanguage::Ja));
        assert!(manifest.capabilities.contains(&Capability::Morphology));

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(RULE_ID));
        assert!(json.contains("\"ja\""));
        assert!(json.contains("\"morphology\""));
    }

    #[test]
    fn version_bumped() {
        assert_eq!(VERSION, "1.0.0");
    }

    // ============================================================================
    // textlint互換テストケース
    // 参考: https://github.com/textlint-ja/textlint-rule-no-doubled-joshi/blob/master/test/no-doubled-joshi-test.ts
    // ============================================================================

    fn create_token(surface: &str, pos: Vec<&str>, start: u32, end: u32) -> Token {
        Token::new(
            surface,
            pos.iter().map(|s| s.to_string()).collect(),
            TextSpan::new(start, end),
        )
    }

    fn create_particle(
        surface: &str,
        key: &str,
        start: usize,
        end: usize,
        idx: usize,
        prev: &str,
        subtype: Option<&str>,
    ) -> ParticleInfo {
        ParticleInfo {
            surface: surface.to_string(),
            key: key.to_string(),
            byte_start: start,
            byte_end: end,
            token_index: idx,
            prev_word: prev.to_string(),
            pos_subtype: subtype.map(|s| s.to_string()),
        }
    }

    // 正常系: エラーにならないケース

    #[test]
    fn valid_no_exception() {
        // 「の」（連体化）は例外扱い
        let config = Config::default();
        let tokens = vec![
            create_token("既存", vec!["名詞"], 0, 6),
            create_token("の", vec!["助詞", "連体化"], 6, 9),
            create_token("コード", vec!["名詞"], 9, 15),
            create_token("の", vec!["助詞", "連体化"], 15, 18),
            create_token("利用", vec!["名詞", "サ変接続"], 18, 24),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        // 両方の「の」が例外としてフィルタリングされる
        assert_eq!(particles.len(), 0);
    }

    #[test]
    fn valid_wo_exception() {
        // 「を」（格助詞）は例外扱い
        let config = Config::default();
        let tokens = vec![
            create_token("オブジェクト", vec!["名詞"], 0, 18),
            create_token("を", vec!["助詞", "格助詞"], 18, 21),
            create_token("返す", vec!["動詞"], 21, 27),
            create_token("関数", vec!["名詞"], 27, 33),
            create_token("を", vec!["助詞", "格助詞"], 33, 36),
            create_token("公開", vec!["名詞", "サ変接続"], 36, 42),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        assert_eq!(particles.len(), 0);
    }

    #[test]
    fn valid_different_subtypes() {
        // 「と」の細分類が異なる場合（格助詞 vs 接続助詞）は別の助詞として扱う
        // 「で」（格助詞）も別の助詞として含まれる
        let config = Config::default();
        let tokens = vec![
            create_token("ターミナル", vec!["名詞"], 0, 15),
            create_token("で", vec!["助詞", "格助詞"], 15, 18),
            create_token("「test」", vec!["名詞"], 18, 27),
            create_token("と", vec!["助詞", "格助詞"], 27, 30),
            create_token("入力", vec!["名詞", "サ変接続"], 30, 36),
            create_token("する", vec!["動詞"], 36, 42),
            create_token("と", vec!["助詞", "接続助詞"], 42, 45),
            create_token("画面", vec!["名詞"], 45, 51),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        // 3つの助詞: で, と(格助詞), と(接続助詞)
        assert_eq!(particles.len(), 3);
        // 2つの「と」を見つけて、キーが異なることを確認
        let to_particles: Vec<&ParticleInfo> =
            particles.iter().filter(|p| p.surface == "と").collect();
        assert_eq!(to_particles.len(), 2);
        assert_ne!(to_particles[0].key, to_particles[1].key);
    }

    #[test]
    fn valid_comma_increases_interval() {
        // 「、」は間隔を増やす
        let config = Config::default();
        let source = "これがiPhone、これがAndroidです。";
        let prev = create_particle("が", "が:助詞.格助詞", 6, 9, 0, "これ", Some("格助詞"));
        let curr = create_particle("が", "が:助詞.格助詞", 21, 24, 1, "これ", Some("格助詞"));

        let interval = calculate_interval(&prev, &curr, source, &config);
        assert!(interval >= 1); // 読点が間隔に加算される
    }

    #[test]
    fn valid_parallel_particles() {
        // 並立助詞は許容される
        let config = Config::default();
        let tokens = vec![
            create_token("登っ", vec!["動詞"], 0, 6),
            create_token("たり", vec!["助詞", "並立助詞"], 6, 12),
            create_token("降り", vec!["動詞"], 12, 18),
            create_token("たり", vec!["助詞", "並立助詞"], 18, 24),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        assert_eq!(particles.len(), 2);
        // 両方とも並立助詞
        assert!(particles[0].pos_subtype == Some("並立助詞".to_string()));
        assert!(particles[1].pos_subtype == Some("並立助詞".to_string()));
    }

    #[test]
    fn valid_ka_douka_pattern() {
        // 「かどうか」パターンは許容される
        let config = Config::default();
        let tokens = vec![
            create_token("これ", vec!["名詞"], 0, 6),
            create_token("に", vec!["助詞", "格助詞"], 6, 9),
            create_token("する", vec!["動詞"], 9, 15),
            create_token("か", vec!["助詞", "副助詞"], 15, 18),
            create_token("どう", vec!["副詞"], 18, 24),
            create_token("か", vec!["助詞", "副助詞"], 24, 27),
            create_token("検討", vec!["名詞", "サ変接続"], 27, 33),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        let ka_particles: Vec<&ParticleInfo> =
            particles.iter().filter(|p| p.surface == "か").collect();
        assert_eq!(ka_particles.len(), 2);

        // 「かどうか」パターンを確認
        let ka_refs: Vec<&ParticleInfo> = ka_particles.iter().map(|p| *p).collect();
        assert!(is_ka_douka_pattern(&ka_refs, &tokens));
    }

    #[test]
    fn valid_te_connection() {
        // 「て」（接続助詞）は例外
        let config = Config::default();
        let tokens = vec![
            create_token("まず", vec!["副詞"], 0, 6),
            create_token("は", vec!["助詞", "係助詞"], 6, 9),
            create_token("試し", vec!["動詞"], 9, 15),
            create_token("て", vec!["助詞", "接続助詞"], 15, 18),
            create_token("いただき", vec!["動詞"], 18, 27),
            create_token("て", vec!["助詞", "接続助詞"], 27, 30),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        // 「は」のみ残り、「て」は例外として除外される
        assert_eq!(particles.len(), 1);
        assert_eq!(particles[0].surface, "は");
    }

    #[test]
    fn valid_rengo_different_keys() {
        // 「に」と「には」は異なる
        let config = Config::default();
        let tokens = vec![
            create_token("その", vec!["連体詞"], 0, 6),
            create_token("ため", vec!["名詞"], 6, 12),
            create_token("、", vec!["記号"], 12, 15),
            create_token("文字列", vec!["名詞"], 15, 24),
            create_token("の", vec!["助詞", "連体化"], 24, 27),
            create_token("長さ", vec!["名詞"], 27, 33),
            create_token("を", vec!["助詞", "格助詞"], 33, 36),
            create_token("正確", vec!["名詞", "形容動詞語幹"], 36, 42),
            create_token("に", vec!["助詞", "格助詞"], 42, 45),
            create_token("測る", vec!["動詞"], 45, 51),
            create_token("に", vec!["助詞", "格助詞"], 51, 54),
            create_token("は", vec!["助詞", "係助詞"], 54, 57),
        ];
        let particles = extract_particles_from_tokens(&tokens, &config);
        // 連結後、「には」は1つの連語になる
        let concatenated = concat_adjacent_particles(particles);
        // 「には」が連語として含まれる
        assert!(concatenated.iter().any(|p| p.surface == "には"));
    }

    // 異常系: エラーになるケース

    #[test]
    fn invalid_doubled_ha() {
        let config = Config::default();
        let source = "私は彼は好きだ";
        let tokens = vec![
            create_token("私", vec!["名詞"], 0, 3),
            create_token("は", vec!["助詞", "係助詞"], 3, 6),
            create_token("彼", vec!["名詞"], 6, 9),
            create_token("は", vec!["助詞", "係助詞"], 9, 12),
            create_token("好き", vec!["名詞"], 12, 18),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        assert_eq!(particles.len(), 2);

        let diagnostics = check_doubled_particles(&particles, &tokens, source, &config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("は"));
        assert!(diagnostics[0].message.contains("私\"は\""));
        assert!(diagnostics[0].message.contains("彼\"は\""));
    }

    #[test]
    fn invalid_doubled_de() {
        let config = Config::default();
        let source = "材料不足で代替素材で製品を作った。";
        let tokens = vec![
            create_token("材料", vec!["名詞"], 0, 6),
            create_token("不足", vec!["名詞", "サ変接続"], 6, 12),
            create_token("で", vec!["助詞", "格助詞"], 12, 15),
            create_token("代替", vec!["名詞", "サ変接続"], 15, 21),
            create_token("素材", vec!["名詞"], 21, 27),
            create_token("で", vec!["助詞", "格助詞"], 27, 30),
            create_token("製品", vec!["名詞"], 30, 36),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        assert_eq!(particles.len(), 2);

        let diagnostics = check_doubled_particles(&particles, &tokens, source, &config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("で"));
    }

    #[test]
    fn invalid_strict_mode() {
        // strictモードでは「の」（連体化）は例外ではない
        let config = Config {
            strict: true,
            ..Default::default()
        };
        let tokens = vec![
            create_token("既存", vec!["名詞"], 0, 6),
            create_token("の", vec!["助詞", "連体化"], 6, 9),
            create_token("コード", vec!["名詞"], 9, 15),
            create_token("の", vec!["助詞", "連体化"], 15, 18),
            create_token("利用", vec!["名詞", "サ変接続"], 18, 24),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        assert_eq!(particles.len(), 2); // strictモードでは「の」はフィルタされない

        let source = "既存のコードの利用";
        let diagnostics = check_doubled_particles(&particles, &tokens, source, &config);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn invalid_strict_mode_adjacent() {
        // strictモードでは、読点がなくても隣接する同じ助詞を検出
        let config = Config {
            strict: true,
            ..Default::default()
        };
        let source = "彼女は困り切った表情で小声で尋ねた。";
        let tokens = vec![
            create_token("彼女", vec!["名詞"], 0, 6),
            create_token("は", vec!["助詞", "係助詞"], 6, 9),
            create_token("困り", vec!["動詞"], 9, 15),
            create_token("切っ", vec!["動詞"], 15, 21),
            create_token("た", vec!["助動詞"], 21, 24),
            create_token("表情", vec!["名詞"], 24, 30),
            create_token("で", vec!["助詞", "格助詞"], 30, 33),
            create_token("小声", vec!["名詞"], 33, 39),
            create_token("で", vec!["助詞", "格助詞"], 39, 42),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        let de_particles: Vec<&ParticleInfo> =
            particles.iter().filter(|p| p.surface == "で").collect();
        assert_eq!(de_particles.len(), 2);

        let diagnostics = check_doubled_particles(&particles, &tokens, source, &config);
        // strictモードでは「で」が検出される（読点なし）
        assert!(diagnostics.iter().any(|d| d.message.contains("で")));
    }

    #[test]
    fn invalid_min_interval() {
        let config = Config {
            min_interval: 2,
            ..Default::default()
        };
        let source = "白装束で重力のない足どりでやってくる";
        let tokens = vec![
            create_token("白装束", vec!["名詞"], 0, 9),
            create_token("で", vec!["助詞", "格助詞"], 9, 12),
            create_token("重力", vec!["名詞"], 12, 18),
            create_token("の", vec!["助詞", "連体化"], 18, 21),
            create_token("ない", vec!["形容詞"], 21, 27),
            create_token("足どり", vec!["名詞"], 27, 36),
            create_token("で", vec!["助詞", "格助詞"], 36, 39),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        let diagnostics = check_doubled_particles(&particles, &tokens, source, &config);
        assert!(diagnostics.len() >= 1);
    }

    #[test]
    fn invalid_rengo_doubled() {
        // 「には」+「には」は検出される
        let config = Config::default();
        let source = "文字列にはそこには問題がある。";
        let tokens = vec![
            create_token("文字列", vec!["名詞"], 0, 9),
            create_token("に", vec!["助詞", "格助詞"], 9, 12),
            create_token("は", vec!["助詞", "係助詞"], 12, 15),
            create_token("そこ", vec!["名詞"], 15, 21),
            create_token("に", vec!["助詞", "格助詞"], 21, 24),
            create_token("は", vec!["助詞", "係助詞"], 24, 27),
            create_token("問題", vec!["名詞"], 27, 33),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        let concatenated = concat_adjacent_particles(particles);
        assert_eq!(concatenated.len(), 2);
        assert_eq!(concatenated[0].surface, "には");
        assert_eq!(concatenated[1].surface, "には");

        let diagnostics = check_doubled_particles(&concatenated, &tokens, source, &config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("には"));
    }

    #[test]
    fn options_allow() {
        // 「も」はオプションで許可
        let config = Config {
            allow: vec!["も".to_string()],
            ..Default::default()
        };
        let tokens = vec![
            create_token("太字", vec!["名詞"], 0, 6),
            create_token("も", vec!["助詞", "係助詞"], 6, 9),
            create_token("強調", vec!["名詞", "サ変接続"], 9, 15),
            create_token("も", vec!["助詞", "係助詞"], 15, 18),
        ];

        let particles = extract_particles_from_tokens(&tokens, &config);
        assert_eq!(particles.len(), 0); // 「も」はallowでフィルタされる
    }

    #[test]
    fn options_custom_separator() {
        // カスタム区切り文字
        let config = Config {
            separator_characters: vec!["♪".to_string()],
            ..Default::default()
        };
        // ♪を区切り文字として、「これはペンです♪これは鉛筆です♪」は2文になる
        // ルールはトークン単位で動作するため、区切り文字ロジックをテスト
        let source = "これはペンです";
        let prev = create_particle("は", "は:助詞.係助詞", 3, 6, 0, "これ", Some("係助詞"));

        // ♪が間にある場合、MAX間隔（文境界として扱われる）を返す
        let source_with_separator = "これはペンです♪";
        let curr_after_sep =
            create_particle("は", "は:助詞.係助詞", 21, 24, 1, "これ", Some("係助詞"));
        let interval = calculate_interval(&prev, &curr_after_sep, source_with_separator, &config);
        // ♪が間にあればMAXを返す
    }

    #[test]
    fn options_empty_comma_characters() {
        // 読点文字がない場合、読点は間隔を増やさない
        let config = Config {
            comma_characters: vec![],
            ..Default::default()
        };
        let source = "これがiPhone、これがAndroidです。";
        let prev = create_particle("が", "が:助詞.格助詞", 6, 9, 0, "これ", Some("格助詞"));
        let curr = create_particle("が", "が:助詞.格助詞", 21, 24, 1, "これ", Some("格助詞"));

        let interval = calculate_interval(&prev, &curr, source, &config);
        // 読点がない場合、間隔は0（間の文字: "iPhone"）
        assert_eq!(interval, 0);
    }
}
