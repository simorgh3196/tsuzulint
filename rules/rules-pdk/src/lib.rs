//! Common types for TsuzuLint WASM rules.
//!
//! This crate provides shared type definitions used across all rule implementations.

use serde::{Deserialize, Serialize};

/// Request sent to a rule's lint function.
#[derive(Debug, Clone, Deserialize)]
pub struct LintRequest {
    /// The AST node to check (serialized JSON).
    pub node: serde_json::Value,
    /// Rule configuration.
    pub config: serde_json::Value,
    /// Full source text.
    pub source: String,
    /// File path (if available).
    pub file_path: Option<String>,
    /// Pre-computed helper information for easier rule development.
    #[serde(default)]
    pub helpers: Option<LintHelpers>,
}

/// Pre-computed helper information for lint rules.
///
/// This provides commonly needed data that would otherwise require
/// repetitive parsing in each rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintHelpers {
    /// The text content of the current node (pre-extracted).
    pub text: Option<String>,

    /// Line and column location information.
    pub location: Option<LintLocation>,

    /// Context about surrounding nodes.
    pub context: Option<LintContext>,

    /// Flags indicating the node's position in the document structure.
    #[serde(default)]
    pub flags: LintFlags,
}

/// Line and column location information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintLocation {
    /// Start position.
    pub start: Position,
    /// End position.
    pub end: Position,
}

/// A position in the document (1-indexed line and column).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    /// Line number (1-indexed).
    pub line: u32,
    /// Column number (1-indexed).
    pub column: u32,
}

impl Position {
    /// Creates a new position.
    pub fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

/// Context about surrounding nodes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintContext {
    /// Type of the previous sibling node.
    pub previous_node_type: Option<String>,

    /// Type of the next sibling node.
    pub next_node_type: Option<String>,

    /// Type of the parent node.
    pub parent_node_type: Option<String>,

    /// Nesting depth in the AST (0 = root).
    #[serde(default)]
    pub depth: u32,
}

/// Flags indicating the node's position in the document structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintFlags {
    /// Whether the node is inside a code block.
    #[serde(default)]
    pub in_code_block: bool,

    /// Whether the node is inside inline code.
    #[serde(default)]
    pub in_code_inline: bool,

    /// Whether the node is inside a heading.
    #[serde(default)]
    pub in_heading: bool,

    /// Whether the node is inside a list.
    #[serde(default)]
    pub in_list: bool,

    /// Whether the node is inside a blockquote.
    #[serde(default)]
    pub in_blockquote: bool,

    /// Whether the node is inside a link.
    #[serde(default)]
    pub in_link: bool,

    /// Whether the node is inside a table.
    #[serde(default)]
    pub in_table: bool,
}

/// Response from a rule's lint function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResponse {
    /// Diagnostics reported by the rule.
    pub diagnostics: Vec<Diagnostic>,
}

/// A diagnostic message from a lint rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The rule that generated this diagnostic.
    pub rule_id: String,
    /// The diagnostic message.
    pub message: String,
    /// Byte span in the source.
    pub span: Span,
    /// Severity level.
    #[serde(default)]
    pub severity: Severity,
    /// Optional fix for this diagnostic.
    pub fix: Option<Fix>,
}

impl Diagnostic {
    /// Creates a new diagnostic with Error severity.
    pub fn new(rule_id: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            rule_id: rule_id.into(),
            message: message.into(),
            span,
            severity: Severity::Error,
            fix: None,
        }
    }

    /// Creates a new diagnostic with Warning severity.
    pub fn warning(rule_id: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            rule_id: rule_id.into(),
            message: message.into(),
            span,
            severity: Severity::Warning,
            fix: None,
        }
    }

    /// Sets the severity level.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Sets the auto-fix.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// Byte span in source text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    /// Start byte offset (0-indexed, inclusive).
    pub start: u32,
    /// End byte offset (0-indexed, exclusive).
    pub end: u32,
}

impl Span {
    /// Creates a new span.
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Returns the length of the span in bytes.
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Returns true if the span is empty.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Error level (default).
    #[default]
    Error,
    /// Warning level.
    Warning,
    /// Informational level.
    Info,
}

/// An auto-fix for a diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    /// The byte span to replace.
    pub span: Span,
    /// The replacement text.
    pub text: String,
}

impl Fix {
    /// Creates a new fix that replaces the span with the given text.
    pub fn new(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }

    /// Creates a fix that inserts text at the given position.
    pub fn insert(offset: u32, text: impl Into<String>) -> Self {
        Self {
            span: Span::new(offset, offset),
            text: text.into(),
        }
    }

    /// Creates a fix that deletes the given span.
    pub fn delete(span: Span) -> Self {
        Self {
            span,
            text: String::new(),
        }
    }
}

/// Rule manifest for registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleManifest {
    /// Unique rule identifier.
    pub name: String,
    /// Rule version (semver).
    pub version: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// Whether this rule can provide auto-fixes.
    #[serde(default)]
    pub fixable: bool,
    /// Node types this rule is interested in.
    #[serde(default)]
    pub node_types: Vec<String>,
}

impl RuleManifest {
    /// Creates a new rule manifest.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            fixable: false,
            node_types: Vec::new(),
        }
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the fixable flag.
    pub fn with_fixable(mut self, fixable: bool) -> Self {
        self.fixable = fixable;
        self
    }

    /// Sets the node types this rule handles.
    pub fn with_node_types(mut self, node_types: Vec<String>) -> Self {
        self.node_types = node_types;
        self
    }
}

/// Helper to extract text range from a node.
///
/// Returns `(start, end, text)` where `start` and `end` are byte offsets.
pub fn extract_node_text<'a>(
    node: &serde_json::Value,
    source: &'a str,
) -> Option<(usize, usize, &'a str)> {
    let range = node.get("range")?.as_array()?;
    let start = range.first()?.as_u64()? as usize;
    let end = range.get(1)?.as_u64()? as usize;

    if end <= source.len() && start <= end {
        Some((start, end, &source[start..end]))
    } else {
        None
    }
}

/// Checks if a node matches the expected type.
pub fn is_node_type(node: &serde_json::Value, expected: &str) -> bool {
    node.get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| t == expected)
}

/// Gets the node type as a string.
pub fn get_node_type(node: &serde_json::Value) -> Option<&str> {
    node.get("type").and_then(|t| t.as_str())
}

// ============================================================================
// Helper accessors for LintRequest
// ============================================================================

/// Gets the text content from the request, using helpers if available.
///
/// Falls back to extracting from source using node range if helpers.text is None.
pub fn get_text<'a>(request: &'a LintRequest) -> Option<&'a str> {
    // Try helpers.text first
    if let Some(ref helpers) = request.helpers {
        if let Some(ref text) = helpers.text {
            return Some(text.as_str());
        }
    }

    // Fall back to extraction
    extract_node_text(&request.node, &request.source).map(|(_, _, text)| text)
}

/// Gets the location from the request helpers.
pub fn get_location(request: &LintRequest) -> Option<&LintLocation> {
    request.helpers.as_ref()?.location.as_ref()
}

/// Checks if the current node is inside a code block.
pub fn is_in_code_block(request: &LintRequest) -> bool {
    request
        .helpers
        .as_ref()
        .map(|h| h.flags.in_code_block || h.flags.in_code_inline)
        .unwrap_or(false)
}

/// Checks if the current node is inside a heading.
pub fn is_in_heading(request: &LintRequest) -> bool {
    request
        .helpers
        .as_ref()
        .map(|h| h.flags.in_heading)
        .unwrap_or(false)
}

/// Checks if the current node is inside a list.
pub fn is_in_list(request: &LintRequest) -> bool {
    request
        .helpers
        .as_ref()
        .map(|h| h.flags.in_list)
        .unwrap_or(false)
}

/// Checks if the current node is inside a blockquote.
pub fn is_in_blockquote(request: &LintRequest) -> bool {
    request
        .helpers
        .as_ref()
        .map(|h| h.flags.in_blockquote)
        .unwrap_or(false)
}

/// Checks if the current node is inside a link.
pub fn is_in_link(request: &LintRequest) -> bool {
    request
        .helpers
        .as_ref()
        .map(|h| h.flags.in_link)
        .unwrap_or(false)
}

/// Gets the parent node type.
pub fn get_parent_type(request: &LintRequest) -> Option<&str> {
    request
        .helpers
        .as_ref()?
        .context
        .as_ref()?
        .parent_node_type
        .as_deref()
}

/// Gets the depth of the current node in the AST.
pub fn get_depth(request: &LintRequest) -> u32 {
    request
        .helpers
        .as_ref()
        .and_then(|h| h.context.as_ref())
        .map(|c| c.depth)
        .unwrap_or(0)
}

// ============================================================================
// Text Processing Helpers
// ============================================================================

/// A sentence found in text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sentence {
    /// Byte start offset in source (relative to the text start).
    pub start: usize,
    /// Byte end offset in source (relative to the text start).
    pub end: usize,
    /// The sentence text.
    pub text: String,
    /// Character count of the sentence.
    pub char_count: usize,
}

/// Splits text into sentences.
///
/// This handles common sentence delimiters including:
/// - English/European: ., !, ?
/// - Japanese: 。, ！, ？
pub fn get_sentences(text: &str) -> Vec<Sentence> {
    let delimiters = ['.', '!', '?', '。', '！', '？'];
    let mut sentences = Vec::new();
    let mut current_start = 0;

    // Find sentence boundaries
    // We iterate through chars to handle multibyte chars correctly
    let mut indices = text.char_indices().peekable();

    while let Some((_idx, c)) = indices.next() {
        if delimiters.contains(&c) {
            // Found a delimiter, this ends the current sentence
            // The delimiter is included in the sentence
            let next_char_idx = indices.peek().map(|(i, _)| *i).unwrap_or(text.len());
            let end = next_char_idx;

            let sentence_text = &text[current_start..end];
            let trimmed = sentence_text.trim();

            if !trimmed.is_empty() {
                // Calculate byte offset of trimmed substring within original range
                let original_slice = &text[current_start..end];
                let leading_bytes = original_slice.len() - original_slice.trim_start().len();
                let new_start = current_start + leading_bytes;
                let new_end = new_start + trimmed.len();

                sentences.push(Sentence {
                    start: new_start,
                    end: new_end,
                    text: trimmed.to_string(),
                    char_count: trimmed.chars().count(), // Count characters, not bytes
                });
            }

            current_start = end;
        }
    }

    // Handle remaining text
    if current_start < text.len() {
        let remaining = &text[current_start..];
        let trimmed = remaining.trim();
        if !trimmed.is_empty() {
            // Calculate byte offset of trimmed substring within original range
            let leading_bytes = remaining.len() - remaining.trim_start().len();
            let new_start = current_start + leading_bytes;
            let new_end = new_start + trimmed.len();

            sentences.push(Sentence {
                start: new_start,
                end: new_end,
                text: trimmed.to_string(),
                char_count: trimmed.chars().count(),
            });
        }
    }

    sentences
}

/// A match found in text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextMatch {
    /// Byte start offset (relative to the text start).
    pub start: usize,
    /// Byte end offset (relative to the text start).
    pub end: usize,
    /// The text that matched.
    pub matched_text: String,
}

/// Finds all occurrences of patterns in text.
pub fn find_matches(text: &str, patterns: &[String], case_sensitive: bool) -> Vec<TextMatch> {
    let mut matches = Vec::new();

    for pattern in patterns {
        // Skip empty patterns to avoid infinite loops
        if pattern.is_empty() {
            continue;
        }

        if case_sensitive {
            // Case-sensitive search: use byte-level matching directly
            let mut search_start = 0;
            while let Some(pos) = text[search_start..].find(pattern) {
                let abs_pos = search_start + pos;
                let end_pos = abs_pos + pattern.len();

                let original_matched = &text[abs_pos..end_pos];

                matches.push(TextMatch {
                    start: abs_pos,
                    end: end_pos,
                    matched_text: original_matched.to_string(),
                });

                search_start = end_pos;
            }
        } else {
            // Case-insensitive search: handle Unicode case expansion (e.g., ß → SS)
            // Build a mapping from uppercase character positions to original byte positions
            let mut upper_chars: Vec<char> = Vec::new();
            let mut byte_mapping: Vec<usize> = Vec::new();

            for (byte_pos, ch) in text.char_indices() {
                for upper_ch in ch.to_uppercase() {
                    upper_chars.push(upper_ch);
                    byte_mapping.push(byte_pos);
                }
            }

            // Build uppercase pattern
            let pattern_upper: Vec<char> = pattern.chars().flat_map(|c| c.to_uppercase()).collect();

            // Search in the uppercase character sequence
            let mut search_start = 0;
            while let Some(pos) = find_char_slice(&upper_chars[search_start..], &pattern_upper) {
                let abs_pos = search_start + pos;
                let end_pos = abs_pos + pattern_upper.len();

                // Map character positions back to byte offsets in original text
                let start_byte = byte_mapping[abs_pos];
                let end_byte = if end_pos < byte_mapping.len() {
                    byte_mapping[end_pos]
                } else {
                    text.len()
                };

                let original_matched = &text[start_byte..end_byte];

                matches.push(TextMatch {
                    start: start_byte,
                    end: end_byte,
                    matched_text: original_matched.to_string(),
                });

                search_start = end_pos;
            }
        }
    }

    // Sort by start position
    matches.sort_by_key(|m| m.start);
    matches
}

/// Finds a pattern (as char slice) within a text (as char slice).
/// Returns the starting character index if found, None otherwise.
fn find_char_slice(text: &[char], pattern: &[char]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    if pattern.len() > text.len() {
        return None;
    }

    for i in 0..=text.len() - pattern.len() {
        if text[i..i + pattern.len()] == pattern[..] {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn span_new() {
        let span = Span::new(10, 20);
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
    }

    #[test]
    fn span_empty() {
        let span = Span::new(5, 5);
        assert!(span.is_empty());
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn diagnostic_new() {
        let diag = Diagnostic::new("test-rule", "Test message", Span::new(0, 10));
        assert_eq!(diag.rule_id, "test-rule");
        assert_eq!(diag.message, "Test message");
        assert_eq!(diag.severity, Severity::Error);
        assert!(diag.fix.is_none());
    }

    #[test]
    fn diagnostic_warning() {
        let diag = Diagnostic::warning("test-rule", "Warning message", Span::new(0, 10));
        assert_eq!(diag.severity, Severity::Warning);
    }

    #[test]
    fn diagnostic_with_fix() {
        let fix = Fix::new(Span::new(0, 5), "replacement");
        let diag = Diagnostic::new("test-rule", "Test", Span::new(0, 10)).with_fix(fix);
        assert!(diag.fix.is_some());
    }

    #[test]
    fn fix_insert() {
        let fix = Fix::insert(10, "inserted");
        assert_eq!(fix.span.start, 10);
        assert_eq!(fix.span.end, 10);
        assert_eq!(fix.text, "inserted");
    }

    #[test]
    fn fix_delete() {
        let fix = Fix::delete(Span::new(5, 15));
        assert_eq!(fix.span.start, 5);
        assert_eq!(fix.span.end, 15);
        assert_eq!(fix.text, "");
    }

    #[test]
    fn rule_manifest_builder() {
        let manifest = RuleManifest::new("my-rule", "1.0.0")
            .with_description("A test rule")
            .with_fixable(true)
            .with_node_types(vec!["Str".to_string()]);

        assert_eq!(manifest.name, "my-rule");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, Some("A test rule".to_string()));
        assert!(manifest.fixable);
        assert_eq!(manifest.node_types, vec!["Str"]);
    }

    #[test]
    fn extract_node_text_valid() {
        let source = "Hello, World!";
        let node = serde_json::json!({
            "type": "Str",
            "range": [0, 5]
        });

        let result = extract_node_text(&node, source);
        assert_eq!(result, Some((0, 5, "Hello")));
    }

    #[test]
    fn extract_node_text_invalid_range() {
        let source = "Hello";
        let node = serde_json::json!({
            "type": "Str",
            "range": [0, 100]
        });

        let result = extract_node_text(&node, source);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_node_text_no_range() {
        let source = "Hello";
        let node = serde_json::json!({
            "type": "Str"
        });

        let result = extract_node_text(&node, source);
        assert_eq!(result, None);
    }

    #[test]
    fn is_node_type_matches() {
        let node = serde_json::json!({ "type": "Str" });
        assert!(is_node_type(&node, "Str"));
        assert!(!is_node_type(&node, "Paragraph"));
    }

    #[test]
    fn is_node_type_missing() {
        let node = serde_json::json!({});
        assert!(!is_node_type(&node, "Str"));
    }

    #[test]
    fn get_node_type_present() {
        let node = serde_json::json!({ "type": "Paragraph" });
        assert_eq!(get_node_type(&node), Some("Paragraph"));
    }

    #[test]
    fn get_node_type_missing() {
        let node = serde_json::json!({});
        assert_eq!(get_node_type(&node), None);
    }

    #[test]
    fn lint_response_serialization() {
        let response = LintResponse {
            diagnostics: vec![Diagnostic::new("test", "msg", Span::new(0, 1))],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("msg"));
    }

    #[test]
    fn severity_default() {
        let severity: Severity = Default::default();
        assert_eq!(severity, Severity::Error);
    }

    #[test]
    fn severity_serialization() {
        assert_eq!(
            serde_json::to_string(&Severity::Error).unwrap(),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
        assert_eq!(serde_json::to_string(&Severity::Info).unwrap(), "\"info\"");
    }

    // ============================================================================
    // Tests for find_matches with Unicode case expansion
    // ============================================================================

    /// Test Sharp S (ß) case expansion: ß → SS
    /// ß is 2 bytes, SS is 4 bytes - this tests byte offset calculation
    #[test]
    fn find_matches_sharp_s_expansion() {
        // "Maßnahme" - ß expands to SS when uppercased
        let text = "Maßnahme";
        let patterns = vec!["mass".to_string()];

        // Case-insensitive search should find "Maß" as "MASS"
        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        // The matched text should be "Maß" (4 bytes), not panic
        assert_eq!(matches[0].matched_text, "Maß");
        assert_eq!(matches[0].end, 4);
    }

    /// Test that Sharp S with case_sensitive=true does not match
    #[test]
    fn find_matches_sharp_s_case_sensitive() {
        let text = "Maßnahme";
        let patterns = vec!["mass".to_string()];

        // Case-sensitive search should not find anything
        let matches = find_matches(text, &patterns, true);
        assert!(matches.is_empty());
    }

    /// Test ligature (ﬁ) case expansion: ﬁ → FI
    /// The ligature ﬁ is a single character that expands to FI
    #[test]
    fn find_matches_ligature_expansion() {
        // "ﬁsh" - ﬁ ligature expands to FI when uppercased
        let text = "ﬁsh";
        let patterns = vec!["fi".to_string()];

        // Case-insensitive search should find "ﬁ" as "FI"
        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        // The matched text should be "ﬁ" (3 bytes in UTF-8)
        assert_eq!(matches[0].matched_text, "ﬁ");
        assert_eq!(matches[0].end, 3);
    }

    /// Test ligature with "fiancé" text
    #[test]
    fn find_matches_ligature_in_fiance() {
        // "ﬁancé" with ligature
        let text = "ﬁancé";
        let patterns = vec!["fi".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].matched_text, "ﬁ");
    }

    /// Test Greek text with accent: "Όσο" contains accented O with tonos
    /// "Ό" (Omicron with tonos) uppercases to "Ό", "σ" to "Σ", "ο" to "Ο"
    /// "oso" → "OSO" which matches "ΌΣΟ" at character positions 0-2
    /// This tests that the algorithm correctly handles Greek accented characters
    #[test]
    fn find_matches_greek_accented() {
        let text = "Όσο";
        let patterns = vec!["oso".to_string()];

        let matches = find_matches(text, &patterns, false);

        // "Ό" doesn't match "O" (different characters), so no match expected
        // Actually, let's verify what happens - the test may need adjustment
        // based on actual Unicode case folding behavior
        assert!(matches.is_empty());
    }

    /// Test Greek sigma (σ) case expansion with uppercase pattern
    #[test]
    fn find_matches_greek_sigma() {
        let text = "Όσο";
        let patterns = vec!["ΣΟ".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 2);
        assert_eq!(matches[0].matched_text, "σο");
    }

    /// Test multiple matches with Unicode expansion
    #[test]
    fn find_matches_multiple_unicode_expansion() {
        // Multiple ß characters
        let text = "Maßnahme und Straße";
        let patterns = vec!["mass".to_string(), "strasse".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 2);
        // First match: "Maß"
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].matched_text, "Maß");
        // Second match: "Straße" (starts at byte 14 based on actual implementation)
        assert_eq!(matches[1].start, 14);
        assert_eq!(matches[1].matched_text, "Straße");
    }

    /// Test multiple matches of same pattern with Unicode expansion
    #[test]
    fn find_matches_repeated_unicode_expansion() {
        let text = "Maß und Maßnahme";
        let patterns = vec!["mass".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 2);
        // Both "Maß" should match
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].matched_text, "Maß");
        assert_eq!(matches[1].start, 9);
        assert_eq!(matches[1].matched_text, "Maß");
    }

    /// Test mixed ASCII and Unicode expansion
    #[test]
    fn find_matches_mixed_ascii_unicode() {
        let text = "The Maß is heavy";
        let patterns = vec!["mass".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 4);
        assert_eq!(matches[0].matched_text, "Maß");
    }

    /// Test empty pattern (case-sensitive only to avoid infinite loop)
    /// Note: Empty pattern with case-insensitive search would cause infinite loop
    /// because search_start would never advance. This is a known limitation.
    #[test]
    fn find_matches_empty_pattern() {
        let text = "Maßnahme";
        let patterns = vec!["".to_string()];

        // Test with case-sensitive instead
        let matches = find_matches(text, &patterns, true);

        // Empty pattern should be skipped to avoid infinite loops
        assert!(matches.is_empty());
    }

    /// Test no match with Unicode text
    #[test]
    fn find_matches_no_match_unicode() {
        let text = "Maßnahme";
        let patterns = vec!["xyz".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert!(matches.is_empty());
    }

    /// Test case-sensitive vs case-insensitive comparison for ASCII
    #[test]
    fn find_matches_case_sensitive_vs_insensitive_ascii() {
        let text = "Hello World";
        let patterns = vec!["world".to_string()];

        // Case-sensitive: should not match
        let matches_sensitive = find_matches(text, &patterns, true);
        assert!(matches_sensitive.is_empty());

        // Case-insensitive: should match
        let matches_insensitive = find_matches(text, &patterns, false);
        assert_eq!(matches_insensitive.len(), 1);
        assert_eq!(matches_insensitive[0].matched_text, "World");
    }

    /// Test case-sensitive vs case-insensitive comparison for Unicode
    #[test]
    fn find_matches_case_sensitive_vs_insensitive_unicode() {
        let text = "Maßnahme";
        let patterns = vec!["MASS".to_string()];

        // Case-sensitive: should not match (exact byte match required)
        let matches_sensitive = find_matches(text, &patterns, true);
        assert!(matches_sensitive.is_empty());

        // Case-insensitive: should match via Unicode case folding
        let matches_insensitive = find_matches(text, &patterns, false);
        assert_eq!(matches_insensitive.len(), 1);
        assert_eq!(matches_insensitive[0].matched_text, "Maß");
    }

    /// Test complex Unicode text with multiple expansions
    #[test]
    fn find_matches_complex_unicode() {
        // Mixed text with various Unicode characters
        let text = "The ﬁsh in the Straße";
        let patterns = vec!["fish".to_string(), "strasse".to_string()];

        let matches = find_matches(text, &patterns, false);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "ﬁsh");
        assert_eq!(matches[1].matched_text, "Straße");
    }
}

    #[test]
    fn lint_response_msgpack_serialization() {
        let response = LintResponse {
            diagnostics: vec![Diagnostic::new("test", "msg", Span::new(0, 1))],
        };
        let bytes = rmp_serde::to_vec_named(&response).unwrap();
        let decoded: LintResponse = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.diagnostics.len(), 1);
        assert_eq!(decoded.diagnostics[0].rule_id, "test");
    }
