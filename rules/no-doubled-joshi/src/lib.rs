//! no-doubled-joshi rule: Detect repeated Japanese particles (助詞).
//!
//! This rule detects when Japanese particles are repeated consecutively,
//! which is typically a grammatical error.
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | particles | string[] | ["は", "が", "を", "に", "で", "と", "も", "の", "へ", "や", "か", "ね", "よ", "な", "ぞ", "わ"] | Particles to check |
//! | min_interval | number | 0 | Minimum distance between same particles (0 = consecutive only) |
//! | allow | string[] | [] | Particles to allow even if doubled |
//! | suggest_fix | boolean | false | Whether to provide auto-fix suggestions |
//!
//! # Example
//!
//! ```json
//! {
//!   "rules": {
//!     "no-doubled-joshi": {
//!       "particles": ["は", "が", "を", "に"],
//!       "suggest_fix": true
//!     }
//!   }
//! }
//! ```

use extism_pdk::*;
use serde::Deserialize;
use tsuzulint_rule_pdk::{
    Diagnostic, Fix, LintRequest, LintResponse, RuleManifest, Span, Token, extract_node_text,
    is_node_type,
};

const RULE_ID: &str = "no-doubled-joshi";
const VERSION: &str = "1.1.0";

const DEFAULT_PARTICLES: &[&str] = &[
    "は", "が", "を", "に", "で", "と", "も", "の", "へ", "や", "か", "ね", "よ", "な", "ぞ", "わ",
];

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    particles: Vec<String>,
    #[serde(default)]
    min_interval: usize,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    suggest_fix: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            min_interval: 0,
            allow: Vec::new(),
            suggest_fix: false,
        }
    }
}

impl Config {
    fn effective_particles(&self) -> Vec<String> {
        let base_particles: Vec<String> = if self.particles.is_empty() {
            DEFAULT_PARTICLES.iter().map(|s| (*s).to_string()).collect()
        } else {
            self.particles.clone()
        };

        base_particles
            .into_iter()
            .filter(|p| !self.allow.contains(p))
            .collect()
    }
}

#[derive(Debug, Clone)]
struct ParticleMatch {
    particle: String,
    byte_start: usize,
    byte_end: usize,
    char_index: usize,
}

fn find_particles_from_tokens(
    tokens: &[Token],
    config: &Config,
    _node_start: u32,
) -> Vec<ParticleMatch> {
    let mut matches = Vec::new();
    let mut char_idx = 0usize;
    let particles = config.effective_particles();

    for token in tokens {
        let is_joshi = token.has_pos("助詞");
        if is_joshi && particles.iter().any(|p| p == &token.surface) {
            matches.push(ParticleMatch {
                particle: token.surface.clone(),
                byte_start: token.span.start as usize,
                byte_end: token.span.end as usize,
                char_index: char_idx,
            });
        }
        char_idx += token.surface.chars().count();
    }

    matches
}

fn find_particles_from_text(
    text: &str,
    particles: &[String],
    base_offset: usize,
) -> Vec<ParticleMatch> {
    let mut matches = Vec::new();
    let mut char_idx = 0;
    let mut byte_offset = 0;

    for c in text.chars() {
        let c_str = c.to_string();
        let char_len = c.len_utf8();

        if particles.contains(&c_str) {
            matches.push(ParticleMatch {
                particle: c_str,
                byte_start: base_offset + byte_offset,
                byte_end: base_offset + byte_offset + char_len,
                char_index: char_idx,
            });
        }

        byte_offset += char_len;
        char_idx += 1;
    }

    matches
}

fn check_doubled_particles(matches: &[ParticleMatch], config: &Config) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut reported_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for i in 1..matches.len() {
        let prev = &matches[i - 1];
        let curr = &matches[i];

        if prev.particle != curr.particle {
            continue;
        }

        let interval = curr.char_index.saturating_sub(prev.char_index + 1);
        if interval > config.min_interval {
            continue;
        }

        if reported_indices.contains(&(i - 1)) {
            continue;
        }

        reported_indices.insert(i - 1);
        reported_indices.insert(i);

        let mut diagnostic = Diagnostic::new(
            RULE_ID,
            format!(
                "Doubled particle '{}' detected. Consider removing the duplicate.",
                curr.particle
            ),
            Span::new(prev.byte_start as u32, curr.byte_end as u32),
        );

        if config.suggest_fix {
            diagnostic = diagnostic.with_fix(Fix::delete(Span::new(
                curr.byte_start as u32,
                curr.byte_end as u32,
            )));
        }

        diagnostics.push(diagnostic);
    }

    diagnostics
}

#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Detect repeated Japanese particles (助詞)")
        .with_fixable(true)
        .with_node_types(vec!["Str".to_string()]);
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

    let node_range = request.node.get("range").and_then(|r| r.as_array());
    let node_start = node_range
        .and_then(|r| r.first().and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let node_end = node_range
        .and_then(|r| r.get(1).and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;

    let particle_matches = if !tokens.is_empty() && node_start < node_end {
        let node_tokens: Vec<Token> = tokens
            .iter()
            .filter(|t| t.span.start >= node_start && t.span.end <= node_end)
            .cloned()
            .collect();
        find_particles_from_tokens(&node_tokens, &config, node_start)
    } else if let Some((start, _end, text)) = extract_node_text(&request.node, &request.source) {
        let particles = config.effective_particles();
        find_particles_from_text(text, &particles, start)
    } else {
        Vec::new()
    };

    diagnostics = check_doubled_particles(&particle_matches, &config);

    Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tsuzulint_rule_pdk::TextSpan;

    #[test]
    fn config_default_particles() {
        let config = Config::default();
        let particles = config.effective_particles();
        assert!(particles.contains(&"は".to_string()));
        assert!(particles.contains(&"が".to_string()));
        assert!(particles.contains(&"を".to_string()));
    }

    #[test]
    fn config_custom_particles() {
        let config = Config {
            particles: vec!["は".to_string(), "が".to_string()],
            ..Default::default()
        };
        let particles = config.effective_particles();
        assert_eq!(particles.len(), 2);
    }

    #[test]
    fn config_allow_particles() {
        let config = Config {
            particles: vec!["は".to_string(), "が".to_string()],
            allow: vec!["は".to_string()],
            ..Default::default()
        };
        let particles = config.effective_particles();
        assert_eq!(particles.len(), 1);
        assert!(!particles.contains(&"は".to_string()));
        assert!(particles.contains(&"が".to_string()));
    }

    #[test]
    fn find_particles_from_text_basic() {
        let particles = vec!["は".to_string(), "が".to_string()];
        let matches = find_particles_from_text("私はは学生", &particles, 0);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].particle, "は");
        assert_eq!(matches[1].particle, "は");
    }

    #[test]
    fn find_particles_from_text_different() {
        let particles = vec!["は".to_string(), "が".to_string()];
        let matches = find_particles_from_text("私は学生が", &particles, 0);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].particle, "は");
        assert_eq!(matches[1].particle, "が");
    }

    #[test]
    fn find_particles_from_text_with_offset() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("私は", &particles, 100);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].byte_start, 103);
        assert_eq!(matches[0].byte_end, 106);
    }

    #[test]
    fn find_particles_from_text_char_indices() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("私はは学生", &particles, 0);

        assert_eq!(matches[0].char_index, 1);
        assert_eq!(matches[1].char_index, 2);
    }

    #[test]
    fn find_particles_from_tokens_joshi() {
        let config = Config::default();
        let tokens = vec![
            Token::new("私", vec!["名詞".to_string()], TextSpan::new(0, 3)),
            Token::new("は", vec!["助詞".to_string()], TextSpan::new(3, 6)),
            Token::new("は", vec!["助詞".to_string()], TextSpan::new(6, 9)),
            Token::new("学生", vec!["名詞".to_string()], TextSpan::new(9, 15)),
        ];

        let matches = find_particles_from_tokens(&tokens, &config, 0);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].particle, "は");
        assert_eq!(matches[0].byte_start, 3);
        assert_eq!(matches[1].particle, "は");
        assert_eq!(matches[1].byte_start, 6);
    }

    #[test]
    fn find_particles_from_tokens_non_joshi() {
        let config = Config::default();
        let tokens = vec![
            Token::new("私", vec!["名詞".to_string()], TextSpan::new(0, 3)),
            Token::new("は", vec!["名詞".to_string()], TextSpan::new(3, 6)),
        ];

        let matches = find_particles_from_tokens(&tokens, &config, 0);

        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn find_particles_from_tokens_with_node_offset() {
        let config = Config::default();
        let tokens = vec![
            Token::new("は", vec!["助詞".to_string()], TextSpan::new(10, 13)),
            Token::new("は", vec!["助詞".to_string()], TextSpan::new(13, 16)),
        ];

        let matches = find_particles_from_tokens(&tokens, &config, 10);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].char_index, 0);
        assert_eq!(matches[1].char_index, 1);
    }

    #[test]
    fn check_doubled_consecutive() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("私はは学生", &particles, 0);
        let config = Config::default();

        let diagnostics = check_doubled_particles(&matches, &config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("は"));
    }

    #[test]
    fn check_doubled_not_consecutive() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("私は学生は", &particles, 0);
        let config = Config {
            min_interval: 0,
            ..Default::default()
        };

        let diagnostics = check_doubled_particles(&matches, &config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn check_doubled_with_interval() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("私は学生は", &particles, 0);
        let config = Config {
            min_interval: 5,
            ..Default::default()
        };

        let diagnostics = check_doubled_particles(&matches, &config);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn check_doubled_different_particles() {
        let particles = vec!["は".to_string(), "が".to_string()];
        let matches = find_particles_from_text("私はが学生", &particles, 0);
        let config = Config::default();

        let diagnostics = check_doubled_particles(&matches, &config);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn check_doubled_with_fix() {
        let particles = vec!["に".to_string()];
        let matches = find_particles_from_text("東京にに行く", &particles, 0);
        let config = Config {
            suggest_fix: true,
            ..Default::default()
        };

        let diagnostics = check_doubled_particles(&matches, &config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_some());
    }

    #[test]
    fn manifest_contains_required_fields() {
        let manifest = RuleManifest::new(RULE_ID, VERSION)
            .with_description("Detect repeated Japanese particles (助詞)")
            .with_fixable(true)
            .with_node_types(vec!["Str".to_string()]);

        assert_eq!(manifest.name, RULE_ID);
        assert_eq!(manifest.version, VERSION);
        assert!(manifest.description.is_some());
        assert!(manifest.fixable);
        assert!(manifest.node_types.contains(&"Str".to_string()));

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(RULE_ID));
    }

    #[test]
    fn no_particles_found() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("Hello World", &particles, 0);
        assert!(matches.is_empty());
    }

    #[test]
    fn empty_text() {
        let particles = vec!["は".to_string()];
        let matches = find_particles_from_text("", &particles, 0);
        assert!(matches.is_empty());
    }

    #[test]
    fn version_bumped() {
        assert_eq!(VERSION, "1.1.0");
    }
}
