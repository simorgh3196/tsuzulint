//! sentence-length rule: Check sentence length.
//!
//! This rule reports sentences that exceed a configurable maximum length,
//! helping to improve readability.
//!
//! # Configuration
//!
//! | Option | Type | Default | Description |
//! |--------|------|---------|-------------|
//! | max | number | 100 | Maximum sentence length in characters |
//! | skip_code | boolean | true | Skip code blocks and inline code |
//!
//! # Example
//!
//! ```json
//! {
//!   "rules": {
//!     "sentence-length": {
//!       "max": 80
//!     }
//!   }
//! }
//! ```

use extism_pdk::*;
use serde::Deserialize;
use texide_rule_foundation::{
    Diagnostic, LintRequest, LintResponse, RuleManifest, Span, extract_node_text, is_node_type,
};

const RULE_ID: &str = "sentence-length";
const VERSION: &str = "1.0.0";
const DEFAULT_MAX_LENGTH: usize = 100;

/// Configuration for the sentence-length rule.
#[derive(Debug, Deserialize)]
struct Config {
    /// Maximum sentence length in characters.
    #[serde(default = "default_max")]
    max: usize,
    /// Skip code blocks and inline code.
    #[serde(default = "default_true")]
    skip_code: bool,
}

fn default_max() -> usize {
    DEFAULT_MAX_LENGTH
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max: DEFAULT_MAX_LENGTH,
            skip_code: true,
        }
    }
}

/// Sentence delimiters for splitting.
const SENTENCE_DELIMITERS: &[char] = &['.', '!', '?', '。', '！', '？'];

/// Represents a sentence with its position in the source.
#[derive(Debug, Clone)]
struct Sentence {
    /// Byte start offset in source.
    byte_start: usize,
    /// Byte end offset in source.
    byte_end: usize,
    /// The sentence text (trimmed).
    text: String,
    /// Character count of the sentence.
    char_count: usize,
}

/// Splits text into sentences and returns sentence information.
fn split_sentences(text: &str, start_offset: usize) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut current_start = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut byte_offset = 0;

    for (_char_idx, &c) in chars.iter().enumerate() {
        let char_len = c.len_utf8();

        if SENTENCE_DELIMITERS.contains(&c) {
            // Include the delimiter in the sentence
            let sentence_byte_end = byte_offset + char_len;
            let sentence_text = &text[current_start..sentence_byte_end];
            let trimmed = sentence_text.trim();

            if !trimmed.is_empty() {
                sentences.push(Sentence {
                    byte_start: start_offset + current_start,
                    byte_end: start_offset + sentence_byte_end,
                    text: trimmed.to_string(),
                    char_count: trimmed.chars().count(),
                });
            }

            current_start = sentence_byte_end;
        }

        byte_offset += char_len;
    }

    // Handle remaining text (sentence without ending punctuation)
    let remaining = &text[current_start..];
    let trimmed = remaining.trim();
    if !trimmed.is_empty() {
        sentences.push(Sentence {
            byte_start: start_offset + current_start,
            byte_end: start_offset + text.len(),
            text: trimmed.to_string(),
            char_count: trimmed.chars().count(),
        });
    }

    sentences
}

/// Returns the rule manifest.
#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {
    let manifest = RuleManifest::new(RULE_ID, VERSION)
        .with_description("Check sentence length")
        .with_fixable(false)
        .with_node_types(vec!["Str".to_string()]);
    Ok(serde_json::to_string(&manifest)?)
}

/// Lints a node for sentence length.
#[plugin_fn]
pub fn lint(input: String) -> FnResult<String> {
    let request: LintRequest = serde_json::from_str(&input)?;
    let mut diagnostics = Vec::new();

    // Only process Str nodes
    if !is_node_type(&request.node, "Str") {
        return Ok(serde_json::to_string(&LintResponse { diagnostics })?);
    }

    // Parse configuration
    let config: Config = serde_json::from_value(request.config.clone()).unwrap_or_default();

    // Extract text from node
    if let Some((start, _end, text)) = extract_node_text(&request.node, &request.source) {
        // Split into sentences
        let sentences = split_sentences(text, start);

        for sentence in sentences {
            if sentence.char_count > config.max {
                diagnostics.push(Diagnostic::warning(
                    RULE_ID,
                    format!(
                        "Sentence is too long ({} characters). Maximum allowed is {}.",
                        sentence.char_count, config.max
                    ),
                    Span::new(sentence.byte_start as u32, sentence.byte_end as u32),
                ));
            }
        }
    }

    Ok(serde_json::to_string(&LintResponse { diagnostics })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn split_sentences_simple() {
        let sentences = split_sentences("Hello. World.", 0);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "Hello.");
        assert_eq!(sentences[1].text, "World.");
    }

    #[test]
    fn split_sentences_japanese() {
        let sentences = split_sentences("これはテストです。次の文です。", 0);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "これはテストです。");
        assert_eq!(sentences[1].text, "次の文です。");
    }

    #[test]
    fn split_sentences_exclamation() {
        let sentences = split_sentences("Wow! Amazing!", 0);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "Wow!");
        assert_eq!(sentences[1].text, "Amazing!");
    }

    #[test]
    fn split_sentences_question() {
        let sentences = split_sentences("What? Why?", 0);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "What?");
        assert_eq!(sentences[1].text, "Why?");
    }

    #[test]
    fn split_sentences_no_delimiter() {
        let sentences = split_sentences("This has no ending", 0);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].text, "This has no ending");
    }

    #[test]
    fn split_sentences_with_offset() {
        let sentences = split_sentences("Test.", 100);
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0].byte_start, 100);
        assert_eq!(sentences[0].byte_end, 105);
    }

    #[test]
    fn split_sentences_empty() {
        let sentences = split_sentences("", 0);
        assert!(sentences.is_empty());
    }

    #[test]
    fn split_sentences_whitespace_only() {
        let sentences = split_sentences("   ", 0);
        assert!(sentences.is_empty());
    }

    #[test]
    fn char_count_ascii() {
        let sentences = split_sentences("Hello.", 0);
        assert_eq!(sentences[0].char_count, 6);
    }

    #[test]
    fn char_count_unicode() {
        let sentences = split_sentences("日本語。", 0);
        // 日本語。 = 4 characters
        assert_eq!(sentences[0].char_count, 4);
    }

    #[test]
    fn config_default() {
        let config = Config::default();
        assert_eq!(config.max, 100);
        assert!(config.skip_code);
    }

    #[test]
    fn manifest_contains_required_fields() {
        // Test manifest structure directly (plugin_fn macro changes signature at compile time)
        let manifest = RuleManifest::new(RULE_ID, VERSION)
            .with_description("Check sentence length")
            .with_fixable(false)
            .with_node_types(vec!["Str".to_string()]);

        assert_eq!(manifest.name, RULE_ID);
        assert_eq!(manifest.version, VERSION);
        assert!(manifest.description.is_some());
        assert!(!manifest.fixable);
        assert!(manifest.node_types.contains(&"Str".to_string()));

        // Verify it serializes correctly
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(RULE_ID));
    }
}
