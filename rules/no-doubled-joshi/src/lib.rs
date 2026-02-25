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
const VERSION: &str = "2.0.0";

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
        assert_eq!(VERSION, "2.0.0");
    }
}
