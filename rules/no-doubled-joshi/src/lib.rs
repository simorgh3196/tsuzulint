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
    Diagnostic, Fix, LintRequest, LintResponse, RuleManifest, Span, extract_node_text, is_node_type,
};

const RULE_ID: &str = "no-doubled-joshi";
const VERSION: &str = "1.0.0";

/// Common Japanese particles (助詞).
const DEFAULT_PARTICLES: &[&str] = &[
    "は", "が", "を", "に", "で", "と", "も", "の", "へ", "や", "か", "ね", "よ", "な", "ぞ", "わ",
];

/// Configuration for the no-doubled-joshi rule.
#[derive(Debug, Deserialize)]
struct Config {
    /// Particles to check (default: common particles).
    #[serde(default)]
    particles: Vec<String>,
    /// Minimum distance (in characters) between same particles to report.
    /// Default 0 means only consecutive particles are reported.
    #[serde(default)]
    min_interval: usize,
    /// Particles to allow even if doubled.
    #[serde(default)]
    allow: Vec<String>,
    /// Whether to provide auto-fix suggestions.
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
    /// Returns the particles to check, filtering out allowed ones.
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

/// Represents a found particle with its position.
#[derive(Debug, Clone)]
struct ParticleMatch {
    /// The particle string.
    particle: String,
    /// Byte start offset in source.
    byte_start: usize,
    /// Byte end offset in source.
    byte_end: usize,
    /// Character index in the text.
    char_index: usize,
}

/// Finds all particles in the text.
fn find_particles(text: &str, particles: &[String], base_offset: usize) -> Vec<ParticleMatch> {
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

/// Checks for doubled particles and returns diagnostics.
fn check_doubled_particles(
    matches: &[ParticleMatch],
    config: &Config,
    _text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // We need to track which matches we've already reported to avoid duplicate reports
    let mut reported_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for i in 1..matches.len() {
        let prev = &matches[i - 1];
        let curr = &matches[i];

        // Check if same particle
        if prev.particle != curr.particle {
            continue;
        }

        // Check interval
        let interval = curr.char_index.saturating_sub(prev.char_index + 1);
        if interval > config.min_interval {
            continue;
        }

        // Skip if already reported
        if reported_indices.contains(&(i - 1)) {
            continue;
        }

        reported_indices.insert(i - 1);
        reported_indices.insert(i);

        // Found doubled particle
        let mut diagnostic = Diagnostic::new(
            RULE_ID,
            format!(
                "Doubled particle '{}' detected. Consider removing the duplicate.",
                curr.particle
            ),
            Span::new(prev.byte_start as u32, curr.byte_end as u32),
        );

        // Provide fix suggestion if enabled
        if config.suggest_fix {
            // Suggest removing the second particle
            diagnostic = diagnostic.with_fix(Fix::delete(Span::new(
                curr.byte_start as u32,
                curr.byte_end as u32,
            )));
        }

        diagnostics.push(diagnostic);
    }

    diagnostics
}

/// Returns the rule manifest.
#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Detect repeated Japanese particles (助詞)")
        .with_fixable(true)
        .with_node_types(vec!["Str".to_string()]);
    Ok(serde_json::to_string(&manifest)?)
}

/// Lints a node for doubled particles.
#[plugin_fn]
pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>> {
    lint_impl(input)
}

fn lint_impl(input: Vec<u8>) -> FnResult<Vec<u8>> {
    let request: LintRequest = rmp_serde::from_slice(&input)?;
    let mut diagnostics = Vec::new();

    // Only process Str nodes
    if !is_node_type(&request.node, "Str") {
        return Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?);
    }

    // Parse configuration
    let config: Config = serde_json::from_value(request.config.clone()).unwrap_or_default();

    // Get effective particles
    let particles = config.effective_particles();

    // Extract text from node
    if let Some((start, _end, text)) = extract_node_text(&request.node, &request.source) {
        // Find all particles
        let particle_matches = find_particles(text, &particles, start);

        // Check for doubled particles
        diagnostics = check_doubled_particles(&particle_matches, &config, text);
    }

    Ok(rmp_serde::to_vec_named(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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
    fn find_particles_basic() {
        let particles = vec!["は".to_string(), "が".to_string()];
        let matches = find_particles("私はは学生", &particles, 0);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].particle, "は");
        assert_eq!(matches[1].particle, "は");
    }

    #[test]
    fn find_particles_different() {
        let particles = vec!["は".to_string(), "が".to_string()];
        let matches = find_particles("私は学生が", &particles, 0);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].particle, "は");
        assert_eq!(matches[1].particle, "が");
    }

    #[test]
    fn find_particles_with_offset() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("私は", &particles, 100);

        assert_eq!(matches.len(), 1);
        // "私" is 3 bytes in UTF-8, so "は" starts at offset 100 + 3 = 103
        assert_eq!(matches[0].byte_start, 103);
        // "は" is 3 bytes in UTF-8, so end is 103 + 3 = 106
        assert_eq!(matches[0].byte_end, 106);
    }

    #[test]
    fn find_particles_char_indices() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("私はは学生", &particles, 0);

        assert_eq!(matches[0].char_index, 1);
        assert_eq!(matches[1].char_index, 2);
    }

    #[test]
    fn check_doubled_consecutive() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("私はは学生", &particles, 0);
        let config = Config::default();

        let diagnostics = check_doubled_particles(&matches, &config, "私はは学生");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("は"));
    }

    #[test]
    fn check_doubled_not_consecutive() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("私は学生は", &particles, 0);
        let config = Config {
            min_interval: 0, // Only consecutive
            ..Default::default()
        };

        let diagnostics = check_doubled_particles(&matches, &config, "私は学生は");
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn check_doubled_with_interval() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("私は学生は", &particles, 0);
        let config = Config {
            min_interval: 5, // Allow up to 5 characters between
            ..Default::default()
        };

        let diagnostics = check_doubled_particles(&matches, &config, "私は学生は");
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn check_doubled_different_particles() {
        let particles = vec!["は".to_string(), "が".to_string()];
        let matches = find_particles("私はが学生", &particles, 0);
        let config = Config::default();

        let diagnostics = check_doubled_particles(&matches, &config, "私はが学生");
        // は and が are different, so no error
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn check_doubled_with_fix() {
        let particles = vec!["に".to_string()];
        let matches = find_particles("東京にに行く", &particles, 0);
        let config = Config {
            suggest_fix: true,
            ..Default::default()
        };

        let diagnostics = check_doubled_particles(&matches, &config, "東京にに行く");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_some());
    }

    #[test]
    fn manifest_contains_required_fields() {
        // Test manifest structure directly (plugin_fn macro changes signature at compile time)
        let manifest = RuleManifest::new(RULE_ID, VERSION)
            .with_description("Detect repeated Japanese particles (助詞)")
            .with_fixable(true)
            .with_node_types(vec!["Str".to_string()]);

        assert_eq!(manifest.name, RULE_ID);
        assert_eq!(manifest.version, VERSION);
        assert!(manifest.description.is_some());
        assert!(manifest.fixable);
        assert!(manifest.node_types.contains(&"Str".to_string()));

        // Verify it serializes correctly
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(RULE_ID));
    }

    #[test]
    fn no_particles_found() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("Hello World", &particles, 0);
        assert!(matches.is_empty());
    }

    #[test]
    fn empty_text() {
        let particles = vec!["は".to_string()];
        let matches = find_particles("", &particles, 0);
        assert!(matches.is_empty());
    }
}
