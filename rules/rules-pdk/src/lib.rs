//! Common types for Texide WASM rules.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Line and column location information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<LintLocation>,

    /// Context about surrounding nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_node_type: Option<String>,

    /// Type of the next sibling node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_node_type: Option<String>,

    /// Type of the parent node.
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
}
