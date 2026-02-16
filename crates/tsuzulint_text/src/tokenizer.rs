use std::ops::Range;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use lindera::tokenizer::Tokenizer as LinderaTokenizer;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum TextError {
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),
}

/// A token representing a morphological unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The surface form of the token (the text itself).
    pub surface: String,
    /// Part of speech (e.g., "名詞", "動詞").
    pub pos: Vec<String>,
    /// Detailed part of speech information.
    pub detail: Vec<String>,
    /// Byte range in the original text.
    pub span: Range<usize>,
}

/// Tokenizer for Japanese text using Lindera.
pub struct Tokenizer {
    inner: LinderaTokenizer,
}

impl std::fmt::Debug for Tokenizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokenizer").finish_non_exhaustive()
    }
}

impl Tokenizer {
    /// Creates a new tokenizer with default IPADIC dictionary.
    pub fn new() -> Result<Self, TextError> {
        let dictionary = load_dictionary("embedded://ipadic")
            .map_err(|e| TextError::Tokenizer(e.to_string()))?;

        let segmenter = Segmenter::new(
            Mode::Normal,
            dictionary,
            None, // user dictionary
        );

        let inner = LinderaTokenizer::new(segmenter);

        Ok(Self { inner })
    }

    /// Tokenizes the given text.
    pub fn tokenize(&self, text: &str) -> Result<Vec<Token>, TextError> {
        let lindera_tokens = self
            .inner
            .tokenize(text)
            .map_err(|e| TextError::Tokenizer(e.to_string()))?;

        let mut tokens = Vec::with_capacity(lindera_tokens.len());

        for mut lindera_token in lindera_tokens {
            let surface = lindera_token.surface.as_ref().to_string();
            let details = lindera_token.details();

            let pos: Vec<String> = details
                .iter()
                .take(4)
                .filter(|s| **s != "*")
                .map(|s| s.to_string())
                .collect();

            let detail: Vec<String> = details
                .iter()
                .skip(4)
                .filter(|s| **s != "*")
                .map(|s| s.to_string())
                .collect();

            let span = lindera_token.byte_start..lindera_token.byte_end;

            tokens.push(Token {
                surface,
                pos,
                detail,
                span,
            });
        }

        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let tokenizer = Tokenizer::new().unwrap();
        let tokens = tokenizer.tokenize("こんにちは世界").unwrap();

        assert_eq!(tokens[0].surface, "こんにちは");
        assert_eq!(tokens[1].surface, "世界");
    }

    #[test]
    fn test_tokenize_empty() {
        let tokenizer = Tokenizer::new().unwrap();
        let tokens = tokenizer.tokenize("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_tokenize_punctuation() {
        let tokenizer = Tokenizer::new().unwrap();
        let tokens = tokenizer.tokenize("。").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].surface, "。");
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokenizer = Tokenizer::new().unwrap();
        let tokens = tokenizer.tokenize("Hello世界").unwrap();
        // "Hello" might be one token or multiple depending on dictionary, but usually one UNK or alphabetic token
        // Lindera default might tokenize "Hello" as "Hello" (UNK) and "世界" as "世界"
        // Let's just check surfaces exist
        let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
        assert!(surfaces.contains(&"Hello"));
        assert!(surfaces.contains(&"世界"));
    }

    #[test]
    fn test_tokenize_long() {
        let tokenizer = Tokenizer::new().unwrap();
        let long_text = "あ".repeat(1000);
        let tokens = tokenizer.tokenize(&long_text).unwrap();
        assert!(!tokens.is_empty());
    }
}
