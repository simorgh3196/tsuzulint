//! Common types for TsuzuLint WASM rules.
//!
//! This crate provides shared type definitions used across all rule implementations.

use extism_pdk::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "extism:host/user")]
unsafe extern "C" {
    fn tsuzulint_get_config(ptr: u64, len: u64) -> u64;
}

#[cfg(not(target_arch = "wasm32"))]
use std::cell::RefCell;

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static MOCK_CONFIG: RefCell<String> = RefCell::new("{}".to_string());
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_mock_config(config: serde_json::Value) {
    MOCK_CONFIG.with(|c| *c.borrow_mut() = config.to_string());
}

/// Helper to get configuration for the current rule.
pub fn get_config<T: DeserializeOwned>() -> FnResult<T> {
    #[cfg(target_arch = "wasm32")]
    {
        // Try Extism config first (safe operation)
        if let Ok(Some(s)) = config::get("config") {
            return Ok(serde_json::from_str(&s)?);
        }

        // Fallback to custom host function (for Wasmi)
        let len = unsafe { tsuzulint_get_config(0, 0) };

        // Check for error sentinel (u64::MAX from -1 i64)
        if len == u64::MAX {
            return Err(Error::msg("Failed to get config: memory write error").into());
        }

        if len == 0 {
            return Ok(serde_json::from_str("{}")?);
        }

        // Allocate buffer and get content
        let mut buf = vec![0u8; len as usize];
        let result = unsafe { tsuzulint_get_config(buf.as_mut_ptr() as u64, len) };

        // Check for error sentinel on second call
        if result == u64::MAX {
            return Err(Error::msg("Failed to get config: memory write error").into());
        }

        let json = String::from_utf8(buf)
            .map_err(|e| Error::msg(format!("Invalid UTF-8 config: {}", e)))?;
        Ok(serde_json::from_str(&json)?)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        MOCK_CONFIG.with(|c| {
            let s = c.borrow();
            Ok(serde_json::from_str(&s)?)
        })
    }
}

/// A minimal AST node representation for lint rules.
///
/// Rules only need the node type and byte range to perform linting.
/// Unknown fields from the host (children, position, etc.) are
/// ignored during deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstNode {
    /// Node type identifier (e.g., "Str", "Paragraph", "Heading").
    #[serde(rename = "type")]
    pub type_: String,
    /// Byte range [start, end] in source text.
    #[serde(default)]
    pub range: Option<[u32; 2]>,
}

impl AstNode {
    /// Creates a new node (used in tests).
    pub fn new(type_: impl Into<String>, range: Option<[u32; 2]>) -> Self {
        Self {
            type_: type_.into(),
            range,
        }
    }
}

#[cfg(test)]
mod ast_node_tests {
    use super::*;

    #[test]
    fn ast_node_deserialize_from_msgpack_with_unknown_fields() {
        // ホストは多くのフィールドを持つノードを送るが、AstNodeは必要なフィールドのみ取得する
        #[derive(Serialize)]
        struct FullNode {
            #[serde(rename = "type")]
            type_: &'static str,
            range: [u32; 2],
            children: Vec<String>, // 無視されるべきフィールド
            value: &'static str,   // 無視されるべきフィールド
        }

        let full = FullNode {
            type_: "Str",
            range: [10, 20],
            children: vec![],
            value: "hello",
        };

        let bytes = rmp_serde::to_vec_named(&full).unwrap();
        let node: AstNode = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(node.type_, "Str");
        assert_eq!(node.range, Some([10, 20]));
    }

    #[test]
    fn ast_node_deserialize_without_range() {
        #[derive(Serialize)]
        struct NodeWithoutRange {
            #[serde(rename = "type")]
            type_: &'static str,
        }

        let node_data = NodeWithoutRange { type_: "Root" };
        let bytes = rmp_serde::to_vec_named(&node_data).unwrap();
        let node: AstNode = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(node.type_, "Root");
        assert_eq!(node.range, None);
    }
}

/// Request sent to a rule's lint function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintRequest {
    /// The AST node to check (serialized JSON) - first node for backward compatibility.
    pub node: serde_json::Value,
    /// All nodes to check in batch mode.
    #[serde(default)]
    pub nodes: Vec<serde_json::Value>,
    /// Rule configuration.
    #[serde(default)]
    pub config: serde_json::Value,
    /// Full source text.
    pub source: String,
    /// File path (if available).
    pub file_path: Option<String>,
    /// Pre-computed helper information for easier rule development.
    #[serde(default)]
    pub helpers: Option<LintHelpers>,
    /// Morphological tokens (from host).
    #[serde(default)]
    pub tokens: Vec<Token>,
    /// Sentences (from host).
    #[serde(default)]
    pub sentences: Vec<TextSentence>,
}

impl LintRequest {
    /// Creates a single-node request (backward compatible).
    pub fn single(node: serde_json::Value, config: serde_json::Value, source: String) -> Self {
        Self {
            node: node.clone(),
            nodes: vec![node],
            config,
            source,
            file_path: None,
            helpers: None,
            tokens: Vec::new(),
            sentences: Vec::new(),
        }
    }

    /// Creates a batch request with multiple nodes.
    pub fn batch(nodes: Vec<serde_json::Value>, config: serde_json::Value, source: String) -> Self {
        Self {
            node: nodes.first().cloned().unwrap_or(serde_json::Value::Null),
            nodes,
            config,
            source,
            file_path: None,
            helpers: None,
            tokens: Vec::new(),
            sentences: Vec::new(),
        }
    }

    /// Returns all nodes (works for both single and batch mode).
    ///
    /// When deserialized from the host (which doesn't include a `nodes` field),
    /// returns a single-element slice containing `self.node`.
    /// Returns an empty slice for empty batch requests where `node` is `Null`.
    pub fn all_nodes(&self) -> &[serde_json::Value] {
        if self.nodes.is_empty() {
            if self.node.is_null() {
                &[]
            } else {
                std::slice::from_ref(&self.node)
            }
        } else {
            &self.nodes
        }
    }

    /// Returns true if this is a batch request with multiple nodes.
    pub fn is_batch(&self) -> bool {
        self.nodes.len() > 1
    }

    /// Sets the file path.
    pub fn with_file_path(mut self, path: Option<impl Into<String>>) -> Self {
        self.file_path = path.map(|p| p.into());
        self
    }

    /// Sets the helpers.
    pub fn with_helpers(mut self, helpers: LintHelpers) -> Self {
        self.helpers = Some(helpers);
        self
    }

    /// Sets the text analysis tokens and sentences.
    pub fn with_text_analysis(mut self, tokens: Vec<Token>, sentences: Vec<TextSentence>) -> Self {
        self.tokens = tokens;
        self.sentences = sentences;
        self
    }

    /// Returns tokens, preferring helpers.text_context if available.
    pub fn get_tokens(&self) -> &[Token] {
        if let Some(ref helpers) = self.helpers {
            if let Some(ref ctx) = helpers.text_context {
                if !ctx.tokens.is_empty() {
                    return &ctx.tokens;
                }
            }
        }
        &self.tokens
    }

    /// Returns sentences, preferring helpers.text_context if available.
    pub fn get_sentences(&self) -> &[TextSentence] {
        if let Some(ref helpers) = self.helpers {
            if let Some(ref ctx) = helpers.text_context {
                if !ctx.sentences.is_empty() {
                    return &ctx.sentences;
                }
            }
        }
        &self.sentences
    }
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

    /// Text analysis context (tokens and sentences).
    #[serde(default)]
    pub text_context: Option<TextContext>,
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

// ============================================================================
// Text Analysis Types (from Core)
// ============================================================================

/// Byte span for text analysis results.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextSpan {
    /// Start byte offset (0-indexed, inclusive).
    pub start: u32,
    /// End byte offset (0-indexed, exclusive).
    pub end: u32,
}

impl TextSpan {
    /// Creates a new text span.
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

/// A morphological token from text analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Token {
    /// The surface form of the token (the text itself).
    pub surface: String,
    /// Part of speech tags (e.g., ["名詞", "一般"]).
    pub pos: Vec<String>,
    /// Detailed morphological information.
    #[serde(default)]
    pub detail: Vec<String>,
    /// Byte span in the source text.
    pub span: TextSpan,
}

impl Token {
    /// Creates a new token.
    pub fn new(surface: impl Into<String>, pos: Vec<String>, span: TextSpan) -> Self {
        Self {
            surface: surface.into(),
            pos,
            detail: Vec::new(),
            span,
        }
    }

    /// Adds detailed information.
    pub fn with_detail(mut self, detail: Vec<String>) -> Self {
        self.detail = detail;
        self
    }

    /// Returns true if this token has the given part of speech.
    pub fn has_pos(&self, pos_tag: &str) -> bool {
        self.pos.iter().any(|p| p == pos_tag)
    }

    // === Part of Speech Accessors ===

    /// Returns the major part of speech (pos[0]).
    /// Example: "動詞", "名詞", "助詞"
    pub fn major_pos(&self) -> Option<&str> {
        self.pos.first().map(|s| s.as_str())
    }

    /// Returns the part of speech detail at the specified level.
    /// level=0: major POS, level=1: detail1, level=2: detail2
    pub fn pos_detail(&self, level: usize) -> Option<&str> {
        self.pos.get(level).map(|s| s.as_str())
    }

    // === Conjugation Accessors (from detail field) ===

    /// Returns the conjugation type (detail[0]).
    /// Example: "五段・ワ行促音便", "一段"
    pub fn conjugation_type(&self) -> Option<&str> {
        self.detail.first().map(|s| s.as_str())
    }

    /// Returns the conjugation form (detail[1]).
    /// Example: "連用形", "基本形", "連体形"
    pub fn conjugation_form(&self) -> Option<&str> {
        self.detail.get(1).map(|s| s.as_str())
    }

    /// Returns the base/dictionary form (detail[2]).
    /// Example: "行う", "食べる"
    pub fn base_form(&self) -> Option<&str> {
        self.detail.get(2).map(|s| s.as_str())
    }

    /// Returns the reading in katakana (detail[3]).
    /// Example: "コト", "オコナウ"
    pub fn reading(&self) -> Option<&str> {
        self.detail.get(3).map(|s| s.as_str())
    }

    // === Common POS Shortcuts ===

    /// Returns true if this token is a verb.
    pub fn is_verb(&self) -> bool {
        self.major_pos() == Some("動詞")
    }

    /// Returns true if this token is a noun.
    pub fn is_noun(&self) -> bool {
        self.major_pos() == Some("名詞")
    }

    /// Returns true if this token is a particle.
    pub fn is_particle(&self) -> bool {
        self.major_pos() == Some("助詞")
    }

    /// Returns true if this token is an auxiliary verb.
    pub fn is_auxiliary_verb(&self) -> bool {
        self.major_pos() == Some("助動詞")
    }

    /// Returns true if this token is an adjective.
    pub fn is_adjective(&self) -> bool {
        self.major_pos() == Some("形容詞")
    }

    /// Returns true if this token is in 連用形 (renyoukei) form.
    pub fn is_renyoukei(&self) -> bool {
        self.conjugation_form() == Some("連用形")
    }
}

/// A sentence from text analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextSentence {
    /// The text content of the sentence.
    pub text: String,
    /// Byte span in the source text.
    pub span: TextSpan,
}

impl TextSentence {
    /// Creates a new sentence.
    pub fn new(text: impl Into<String>, span: TextSpan) -> Self {
        Self {
            text: text.into(),
            span,
        }
    }
}

/// Text analysis context provided by the linter core.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextContext {
    /// Morphological tokens from text analysis.
    #[serde(default)]
    pub tokens: Vec<Token>,
    /// Sentences from text analysis.
    #[serde(default)]
    pub sentences: Vec<TextSentence>,
}

impl TextContext {
    /// Creates an empty text context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a text context with tokens and sentences.
    pub fn with_analysis(tokens: Vec<Token>, sentences: Vec<TextSentence>) -> Self {
        Self { tokens, sentences }
    }

    /// Returns tokens that have the given part of speech.
    pub fn tokens_with_pos(&self, pos_tag: &str) -> Vec<&Token> {
        self.tokens.iter().filter(|t| t.has_pos(pos_tag)).collect()
    }

    /// Returns true if there are no tokens or sentences.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.sentences.is_empty()
    }
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

// ============================================================================
// Language and Capability Types
// ============================================================================

/// Supported languages for text analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KnownLanguage {
    #[default]
    Ja,
    En,
    // Zh, // Chinese (not yet implemented)
    // Ko, // Korean (not yet implemented)
}

/// Required analysis capabilities for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// Morphological analysis (tokenization).
    Morphology,
    /// Sentence boundary detection.
    Sentences,
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
    /// Supported languages.
    #[serde(default)]
    pub languages: Vec<KnownLanguage>,
    /// Required analysis capabilities.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
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
            languages: Vec::new(),
            capabilities: Vec::new(),
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

    /// Sets the supported languages.
    pub fn with_languages(mut self, languages: Vec<KnownLanguage>) -> Self {
        self.languages = languages;
        self
    }

    /// Sets the required analysis capabilities.
    pub fn with_capabilities(mut self, capabilities: Vec<Capability>) -> Self {
        self.capabilities = capabilities;
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

    #[cfg(not(target_arch = "wasm32"))]
    mod mock_config_tests {
        use super::{Deserialize, Diagnostic, LintResponse, Span, get_config, set_mock_config};
        use rmp_serde;

        #[derive(Debug, Deserialize)]
        struct TestConfig {
            #[serde(default)]
            key: String,
        }

        #[test]
        fn test_get_config_valid() {
            set_mock_config(serde_json::json!({"key": "value"}));
            let config: TestConfig = get_config().expect("Failed to get config");
            assert_eq!(config.key, "value");
            set_mock_config(serde_json::json!({}));
        }

        #[test]
        fn test_get_config_empty() {
            set_mock_config(serde_json::json!({}));
            let config: TestConfig = get_config().expect("Failed to get empty config");
            assert_eq!(config.key, "");
        }

        #[test]
        fn test_get_config_invalid_json() {
            #[derive(Debug, Deserialize)]
            struct StrictConfig {
                required_field: String,
            }

            set_mock_config(serde_json::json!({}));
            let result: Result<StrictConfig, _> = get_config();
            assert!(result.is_err());
        }
    }

    #[test]
    fn lint_request_single() {
        let node = serde_json::json!({"type": "Str", "range": [0, 5]});
        let config = serde_json::json!({"option": "value"});
        let source = "test source".to_string();

        let request = LintRequest::single(node.clone(), config.clone(), source.clone());

        assert_eq!(request.node, node);
        assert_eq!(request.nodes.len(), 1);
        assert_eq!(request.nodes[0], node);
        assert!(!request.is_batch());
        assert_eq!(request.all_nodes().len(), 1);
    }

    #[test]
    fn lint_request_batch() {
        let nodes = vec![
            serde_json::json!({"type": "Str", "range": [0, 5]}),
            serde_json::json!({"type": "Str", "range": [10, 15]}),
            serde_json::json!({"type": "Str", "range": [20, 25]}),
        ];
        let config = serde_json::json!({"option": "value"});
        let source = "test source".to_string();

        let request = LintRequest::batch(nodes.clone(), config.clone(), source.clone());

        assert_eq!(request.node, nodes[0]);
        assert_eq!(request.nodes.len(), 3);
        assert!(request.is_batch());
        assert_eq!(request.all_nodes().len(), 3);
    }

    #[test]
    fn lint_request_with_file_path() {
        let node = serde_json::json!({"type": "Str"});
        let request = LintRequest::single(node, serde_json::json!({}), "source".to_string())
            .with_file_path(Some("test.md"));

        assert_eq!(request.file_path, Some("test.md".to_string()));
    }

    #[test]
    fn lint_request_with_helpers() {
        let node = serde_json::json!({"type": "Str"});
        let helpers = LintHelpers {
            text: Some("sample text".to_string()),
            ..Default::default()
        };
        let request = LintRequest::single(node, serde_json::json!({}), "source".to_string())
            .with_helpers(helpers);

        assert!(request.helpers.is_some());
        assert_eq!(
            request.helpers.as_ref().unwrap().text,
            Some("sample text".to_string())
        );
    }

    #[test]
    fn lint_request_batch_empty() {
        let request = LintRequest::batch(vec![], serde_json::json!({}), "source".to_string());

        assert!(!request.is_batch());
        assert_eq!(request.node, serde_json::Value::Null);
        assert_eq!(request.nodes.len(), 0);
        // Empty batch should return empty slice, not [Null]
        assert_eq!(request.all_nodes().len(), 0);
        assert!(request.all_nodes().is_empty());
    }

    #[test]
    fn lint_request_batch_single_node_is_not_batch() {
        let node = serde_json::json!({"type": "Str", "range": [0, 5]});
        let request = LintRequest::batch(
            vec![node.clone()],
            serde_json::json!({}),
            "source".to_string(),
        );

        assert_eq!(request.node, node);
        assert_eq!(request.nodes.len(), 1);
        // A batch() with exactly one node is not considered a batch (nodes.len() > 1 is false).
        assert!(!request.is_batch());
        assert_eq!(request.all_nodes().len(), 1);
    }

    #[test]
    fn lint_request_msgpack_roundtrip_single() {
        let node = serde_json::json!({"type": "Str", "range": [0, 5]});
        let request = LintRequest::single(
            node.clone(),
            serde_json::json!({"opt": 42}),
            "test".to_string(),
        );

        let bytes = rmp_serde::to_vec_named(&request).unwrap();
        let decoded: LintRequest = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(decoded.node, node);
        assert_eq!(decoded.nodes.len(), 1);
        assert_eq!(decoded.source, "test");
    }

    #[test]
    fn lint_request_msgpack_roundtrip_batch() {
        let nodes = vec![
            serde_json::json!({"type": "Str", "range": [0, 5]}),
            serde_json::json!({"type": "Str", "range": [10, 15]}),
        ];
        let request = LintRequest::batch(
            nodes.clone(),
            serde_json::json!({"opt": 42}),
            "test".to_string(),
        );

        let bytes = rmp_serde::to_vec_named(&request).unwrap();
        let decoded: LintRequest = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(decoded.node, nodes[0]);
        assert_eq!(decoded.nodes.len(), 2);
        assert!(decoded.is_batch());
    }

    /// Test `all_nodes()` returns single node when deserialized without `nodes` field.
    /// This simulates the host sending data without the `nodes` field.
    #[test]
    fn lint_request_all_nodes_without_nodes_field() {
        use serde::ser::SerializeMap;

        struct HostLintRequest {
            node: serde_json::Value,
            config: serde_json::Value,
            source: String,
            file_path: Option<String>,
        }

        impl serde::Serialize for HostLintRequest {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("node", &self.node)?;
                map.serialize_entry("config", &self.config)?;
                map.serialize_entry("source", &self.source)?;
                map.serialize_entry("file_path", &self.file_path)?;
                map.end()
            }
        }

        let host_request = HostLintRequest {
            node: serde_json::json!({"type": "Str", "range": [0, 5]}),
            config: serde_json::json!({"opt": 42}),
            source: "test".to_string(),
            file_path: Some("test.md".to_string()),
        };

        let bytes = rmp_serde::to_vec_named(&host_request).unwrap();
        let decoded: LintRequest = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(decoded.node, host_request.node);
        assert!(decoded.nodes.is_empty());

        assert_eq!(decoded.all_nodes().len(), 1);
        assert_eq!(decoded.all_nodes()[0], host_request.node);
        assert!(!decoded.is_batch());
    }

    /// Test `all_nodes()` works correctly when `nodes` is manually empty but `node` has a value.
    #[test]
    fn lint_request_all_nodes_empty_nodes_with_valid_node() {
        let request = LintRequest {
            node: serde_json::json!({"type": "Str", "range": [0, 5]}),
            nodes: vec![],
            config: serde_json::json!({}),
            source: "test".to_string(),
            file_path: None,
            helpers: None,
            tokens: Vec::new(),
            sentences: Vec::new(),
        };

        assert!(request.nodes.is_empty());
        assert_eq!(request.all_nodes().len(), 1);
        assert_eq!(request.all_nodes()[0]["type"], "Str");
        assert!(!request.is_batch());
    }

    /// Test `all_nodes()` with JSON deserialization (without `nodes` field).
    #[test]
    fn lint_request_all_nodes_json_without_nodes_field() {
        let json = r#"{
            "node": {"type": "Str", "range": [0, 5]},
            "config": {"opt": 42},
            "source": "test"
        }"#;

        let decoded: LintRequest = serde_json::from_str(json).unwrap();

        assert_eq!(decoded.node["type"], "Str");
        assert!(decoded.nodes.is_empty());

        assert_eq!(decoded.all_nodes().len(), 1);
        assert_eq!(decoded.all_nodes()[0]["type"], "Str");
        assert!(!decoded.is_batch());
    }

    // ============================================================================
    // Tests for TextContext types
    // ============================================================================

    #[test]
    fn text_span_new() {
        let span = TextSpan::new(10, 20);
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
    }

    #[test]
    fn text_span_empty() {
        let span = TextSpan::new(5, 5);
        assert!(span.is_empty());
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn token_new() {
        let token = Token::new("は", vec!["助詞".to_string()], TextSpan::new(0, 3));
        assert_eq!(token.surface, "は");
        assert_eq!(token.pos, vec!["助詞"]);
        assert!(token.detail.is_empty());
        assert!(token.has_pos("助詞"));
        assert!(!token.has_pos("名詞"));
    }

    #[test]
    fn token_with_detail() {
        let token = Token::new("は", vec!["助詞".to_string()], TextSpan::new(0, 3))
            .with_detail(vec!["係助詞".to_string(), "*".to_string()]);
        assert_eq!(token.detail, vec!["係助詞", "*"]);
    }

    #[test]
    fn text_sentence_new() {
        let sentence = TextSentence::new("こんにちは。", TextSpan::new(0, 18));
        assert_eq!(sentence.text, "こんにちは。");
        assert_eq!(sentence.span.start, 0);
        assert_eq!(sentence.span.end, 18);
    }

    #[test]
    fn text_context_new() {
        let ctx = TextContext::new();
        assert!(ctx.is_empty());
        assert!(ctx.tokens.is_empty());
        assert!(ctx.sentences.is_empty());
    }

    #[test]
    fn text_context_with_analysis() {
        let tokens = vec![Token::new(
            "は",
            vec!["助詞".to_string()],
            TextSpan::new(0, 3),
        )];
        let sentences = vec![TextSentence::new("テスト。", TextSpan::new(0, 12))];
        let ctx = TextContext::with_analysis(tokens.clone(), sentences.clone());

        assert!(!ctx.is_empty());
        assert_eq!(ctx.tokens.len(), 1);
        assert_eq!(ctx.sentences.len(), 1);
    }

    #[test]
    fn text_context_tokens_with_pos() {
        let tokens = vec![
            Token::new("私", vec!["名詞".to_string()], TextSpan::new(0, 3)),
            Token::new("は", vec!["助詞".to_string()], TextSpan::new(3, 6)),
            Token::new("学生", vec!["名詞".to_string()], TextSpan::new(6, 12)),
        ];
        let ctx = TextContext::with_analysis(tokens, vec![]);

        let nouns = ctx.tokens_with_pos("名詞");
        assert_eq!(nouns.len(), 2);

        let particles = ctx.tokens_with_pos("助詞");
        assert_eq!(particles.len(), 1);
    }

    #[test]
    fn text_span_msgpack_roundtrip() {
        let span = TextSpan::new(10, 25);
        let bytes = rmp_serde::to_vec_named(&span).unwrap();
        let decoded: TextSpan = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded, span);
    }

    #[test]
    fn token_msgpack_roundtrip() {
        let token = Token::new(
            "は",
            vec!["助詞".to_string(), "係助詞".to_string()],
            TextSpan::new(3, 6),
        )
        .with_detail(vec!["*".to_string(), "*".to_string()]);
        let bytes = rmp_serde::to_vec_named(&token).unwrap();
        let decoded: Token = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.surface, token.surface);
        assert_eq!(decoded.pos, token.pos);
        assert_eq!(decoded.span, token.span);
    }

    #[test]
    fn text_sentence_msgpack_roundtrip() {
        let sentence = TextSentence::new("こんにちは世界。", TextSpan::new(0, 24));
        let bytes = rmp_serde::to_vec_named(&sentence).unwrap();
        let decoded: TextSentence = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.text, sentence.text);
        assert_eq!(decoded.span, sentence.span);
    }

    #[test]
    fn text_context_msgpack_roundtrip() {
        let tokens = vec![
            Token::new("私", vec!["名詞".to_string()], TextSpan::new(0, 3)),
            Token::new("は", vec!["助詞".to_string()], TextSpan::new(3, 6)),
        ];
        let sentences = vec![TextSentence::new("私は学生。", TextSpan::new(0, 18))];
        let ctx = TextContext::with_analysis(tokens, sentences);

        let bytes = rmp_serde::to_vec_named(&ctx).unwrap();
        let decoded: TextContext = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(decoded.tokens.len(), 2);
        assert_eq!(decoded.sentences.len(), 1);
        assert_eq!(decoded.tokens[0].surface, "私");
        assert_eq!(decoded.tokens[1].surface, "は");
    }

    #[test]
    fn lint_helpers_with_text_context() {
        let text_ctx = TextContext::with_analysis(
            vec![Token::new(
                "は",
                vec!["助詞".to_string()],
                TextSpan::new(0, 3),
            )],
            vec![],
        );
        let helpers = LintHelpers {
            text: Some("テスト".to_string()),
            text_context: Some(text_ctx),
            ..Default::default()
        };

        assert!(helpers.text_context.is_some());
        let ctx = helpers.text_context.unwrap();
        assert_eq!(ctx.tokens.len(), 1);
        assert!(ctx.tokens[0].has_pos("助詞"));
    }

    #[test]
    fn lint_request_with_text_context_msgpack_roundtrip() {
        let text_ctx = TextContext::with_analysis(
            vec![Token::new(
                "は",
                vec!["助詞".to_string()],
                TextSpan::new(3, 6),
            )],
            vec![TextSentence::new("私は学生。", TextSpan::new(0, 18))],
        );
        let helpers = LintHelpers {
            text_context: Some(text_ctx),
            ..Default::default()
        };
        let request = LintRequest::single(
            serde_json::json!({"type": "Str", "range": [0, 18]}),
            serde_json::json!({}),
            "私は学生。".to_string(),
        )
        .with_helpers(helpers);

        let bytes = rmp_serde::to_vec_named(&request).unwrap();
        let decoded: LintRequest = rmp_serde::from_slice(&bytes).unwrap();

        assert!(decoded.helpers.is_some());
        let ctx = decoded.helpers.unwrap().text_context.unwrap();
        assert_eq!(ctx.tokens.len(), 1);
        assert_eq!(ctx.sentences.len(), 1);
        assert_eq!(ctx.tokens[0].surface, "は");
    }

    // ============================================================================
    // Tests for Token Accessors
    // ============================================================================

    #[test]
    fn token_major_pos() {
        let token = Token::new(
            "は",
            vec!["助詞".to_string(), "係助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert_eq!(token.major_pos(), Some("助詞"));
    }

    #[test]
    fn token_pos_detail() {
        let token = Token::new(
            "は",
            vec!["助詞".to_string(), "係助詞".to_string()],
            TextSpan::new(0, 3),
        );
        assert_eq!(token.pos_detail(0), Some("助詞"));
        assert_eq!(token.pos_detail(1), Some("係助詞"));
        assert_eq!(token.pos_detail(2), None);
    }

    #[test]
    fn token_conjugation_accessors() {
        let token =
            Token::new("行う", vec!["動詞".to_string()], TextSpan::new(0, 9)).with_detail(vec![
                "五段・ワ行促音便".to_string(),
                "連用形".to_string(),
                "行う".to_string(),
                "オコナウ".to_string(),
            ]);
        assert_eq!(token.conjugation_type(), Some("五段・ワ行促音便"));
        assert_eq!(token.conjugation_form(), Some("連用形"));
        assert_eq!(token.base_form(), Some("行う"));
        assert_eq!(token.reading(), Some("オコナウ"));
    }

    #[test]
    fn token_accessors_empty_pos_and_detail_return_none() {
        let token = Token::new("x", vec![], TextSpan::new(0, 1));
        assert!(token.major_pos().is_none());
        assert!(token.pos_detail(0).is_none());
        assert!(token.pos_detail(1).is_none());
        assert!(token.conjugation_type().is_none());
        assert!(token.conjugation_form().is_none());
        assert!(token.base_form().is_none());
        assert!(token.reading().is_none());
        assert!(!token.is_verb());
        assert!(!token.is_renyoukei());
    }

    #[test]
    fn token_pos_shortcuts() {
        let verb = Token::new("行う", vec!["動詞".to_string()], TextSpan::new(0, 9));
        assert!(verb.is_verb());
        assert!(!verb.is_noun());
        assert!(!verb.is_particle());

        let noun = Token::new("私", vec!["名詞".to_string()], TextSpan::new(0, 3));
        assert!(noun.is_noun());
        assert!(!noun.is_verb());

        let particle = Token::new("は", vec!["助詞".to_string()], TextSpan::new(0, 3));
        assert!(particle.is_particle());
        assert!(!particle.is_verb());

        let aux_verb = Token::new("だ", vec!["助動詞".to_string()], TextSpan::new(0, 3));
        assert!(aux_verb.is_auxiliary_verb());
        assert!(!aux_verb.is_verb());

        let adjective = Token::new("良い", vec!["形容詞".to_string()], TextSpan::new(0, 6));
        assert!(adjective.is_adjective());
    }

    #[test]
    fn token_is_renyoukei() {
        let renyoukei = Token::new("行き", vec!["動詞".to_string()], TextSpan::new(0, 6))
            .with_detail(vec!["五段".to_string(), "連用形".to_string()]);
        assert!(renyoukei.is_renyoukei());

        let not_renyoukei = Token::new("行う", vec!["動詞".to_string()], TextSpan::new(0, 9))
            .with_detail(vec!["五段".to_string(), "基本形".to_string()]);
        assert!(!not_renyoukei.is_renyoukei());
    }

    // ============================================================================
    // Tests for KnownLanguage and Capability
    // ============================================================================

    #[test]
    fn known_language_serialization() {
        assert_eq!(serde_json::to_string(&KnownLanguage::Ja).unwrap(), "\"ja\"");
        assert_eq!(serde_json::to_string(&KnownLanguage::En).unwrap(), "\"en\"");
    }

    #[test]
    fn known_language_default_is_japanese() {
        assert_eq!(KnownLanguage::default(), KnownLanguage::Ja);
    }

    #[test]
    fn known_language_deserialization() {
        let lang: KnownLanguage = serde_json::from_str("\"ja\"").unwrap();
        assert_eq!(lang, KnownLanguage::Ja);

        let lang: KnownLanguage = serde_json::from_str("\"en\"").unwrap();
        assert_eq!(lang, KnownLanguage::En);
    }

    #[test]
    fn capability_serialization() {
        assert_eq!(
            serde_json::to_string(&Capability::Morphology).unwrap(),
            "\"morphology\""
        );
        assert_eq!(
            serde_json::to_string(&Capability::Sentences).unwrap(),
            "\"sentences\""
        );
    }

    #[test]
    fn capability_deserialization() {
        let cap: Capability = serde_json::from_str("\"morphology\"").unwrap();
        assert_eq!(cap, Capability::Morphology);

        let cap: Capability = serde_json::from_str("\"sentences\"").unwrap();
        assert_eq!(cap, Capability::Sentences);
    }

    #[test]
    fn rule_manifest_with_languages_and_capabilities() {
        let manifest = RuleManifest::new("test-rule", "1.0.0")
            .with_languages(vec![KnownLanguage::Ja])
            .with_capabilities(vec![Capability::Morphology]);

        assert_eq!(manifest.languages, vec![KnownLanguage::Ja]);
        assert_eq!(manifest.capabilities, vec![Capability::Morphology]);
    }

    #[test]
    fn rule_manifest_json_with_languages_and_capabilities() {
        let manifest = RuleManifest::new("test-rule", "1.0.0")
            .with_languages(vec![KnownLanguage::Ja, KnownLanguage::En])
            .with_capabilities(vec![Capability::Morphology, Capability::Sentences]);

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"languages\":[\"ja\",\"en\"]"));
        assert!(json.contains("\"capabilities\":[\"morphology\",\"sentences\"]"));
    }

    #[test]
    fn rule_manifest_deserialize_with_languages_and_capabilities() {
        let json = r#"{
            "name": "test-rule",
            "version": "1.0.0",
            "languages": ["ja"],
            "capabilities": ["morphology"]
        }"#;

        let manifest: RuleManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "test-rule");
        assert_eq!(manifest.languages, vec![KnownLanguage::Ja]);
        assert_eq!(manifest.capabilities, vec![Capability::Morphology]);
    }

    #[test]
    fn rule_manifest_defaults_empty_languages_and_capabilities() {
        let manifest = RuleManifest::new("test", "1.0.0");
        assert!(manifest.languages.is_empty());
        assert!(manifest.capabilities.is_empty());
    }
}
