//! Diagnostic types for lint results.

use serde::{Deserialize, Serialize};
use tsuzulint_ast::{Location, Span};

/// Severity level for diagnostics.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
// #[cfg_attr(feature = "rkyv", rkyv(check_bytes))]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Error - must be fixed.
    #[default]
    Error,
    /// Warning - should be reviewed.
    Warning,
    /// Info - informational message.
    Info,
}

/// A diagnostic message from a lint rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
// #[cfg_attr(feature = "rkyv", rkyv(check_bytes))]
pub struct Diagnostic {
    /// The rule that generated this diagnostic.
    pub rule_id: String,

    /// The diagnostic message.
    pub message: String,

    /// Byte span in the source.
    pub span: Span,

    /// Line/column location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc: Option<Location>,

    /// Severity level.
    #[serde(default)]
    pub severity: Severity,

    /// Optional fix for this diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

impl Diagnostic {
    /// Creates a new diagnostic.
    pub fn new(rule_id: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            rule_id: rule_id.into(),
            message: message.into(),
            span,
            loc: None,
            severity: Severity::Error,
            fix: None,
        }
    }

    /// Sets the severity level.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Sets the location.
    pub fn with_location(mut self, loc: Location) -> Self {
        self.loc = Some(loc);
        self
    }

    /// Sets an auto-fix.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// An auto-fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
// #[cfg_attr(feature = "rkyv", rkyv(check_bytes))]
pub struct Fix {
    /// The byte span to replace.
    pub span: Span,

    /// The replacement text.
    pub text: String,
}

impl Fix {
    /// Creates a new fix.
    pub fn new(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }

    /// Creates a fix that inserts text at a position.
    pub fn insert(offset: u32, text: impl Into<String>) -> Self {
        Self {
            span: Span::new(offset, offset),
            text: text.into(),
        }
    }

    /// Creates a fix that deletes a span.
    pub fn delete(span: Span) -> Self {
        Self {
            span,
            text: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_new() {
        let diag = Diagnostic::new("no-todo", "Found TODO", Span::new(0, 4));

        assert_eq!(diag.rule_id, "no-todo");
        assert_eq!(diag.message, "Found TODO");
        assert_eq!(diag.severity, Severity::Error);
    }

    #[test]
    fn test_diagnostic_with_fix() {
        let fix = Fix::new(Span::new(0, 4), "DONE");
        let diag = Diagnostic::new("no-todo", "Found TODO", Span::new(0, 4)).with_fix(fix);

        assert!(diag.fix.is_some());
        assert_eq!(diag.fix.as_ref().unwrap().text, "DONE");
    }

    #[test]
    fn test_fix_insert() {
        let fix = Fix::insert(10, "inserted");

        assert_eq!(fix.span.start, 10);
        assert_eq!(fix.span.end, 10);
        assert_eq!(fix.text, "inserted");
    }

    #[test]
    fn test_fix_delete() {
        let fix = Fix::delete(Span::new(5, 15));

        assert_eq!(fix.span.start, 5);
        assert_eq!(fix.span.end, 15);
        assert!(fix.text.is_empty());
    }

    #[test]
    fn test_diagnostic_with_severity() {
        let diag =
            Diagnostic::new("rule", "message", Span::new(0, 5)).with_severity(Severity::Warning);

        assert_eq!(diag.severity, Severity::Warning);
    }

    #[test]
    fn test_diagnostic_with_location() {
        use tsuzulint_ast::Position;
        let loc = Location::new(Position::new(1, 1), Position::new(1, 10));
        let diag = Diagnostic::new("rule", "message", Span::new(0, 10)).with_location(loc);

        assert!(diag.loc.is_some());
        let location = diag.loc.unwrap();
        assert_eq!(location.start.line, 1);
        assert_eq!(location.start.column, 1);
        assert_eq!(location.end.line, 1);
        assert_eq!(location.end.column, 10);
    }

    #[test]
    fn test_severity_default() {
        let severity = Severity::default();
        assert_eq!(severity, Severity::Error);
    }

    #[test]
    fn test_severity_equality() {
        assert_eq!(Severity::Error, Severity::Error);
        assert_eq!(Severity::Warning, Severity::Warning);
        assert_eq!(Severity::Info, Severity::Info);
        assert_ne!(Severity::Error, Severity::Warning);
    }

    #[test]
    fn test_diagnostic_builder_chain() {
        use tsuzulint_ast::Position;
        let fix = Fix::new(Span::new(0, 4), "fixed");
        let loc = Location::new(Position::new(1, 1), Position::new(1, 5));

        let diag = Diagnostic::new("rule-id", "Error message", Span::new(0, 4))
            .with_severity(Severity::Warning)
            .with_location(loc)
            .with_fix(fix);

        assert_eq!(diag.rule_id, "rule-id");
        assert_eq!(diag.message, "Error message");
        assert_eq!(diag.severity, Severity::Warning);
        assert!(diag.loc.is_some());
        assert!(diag.fix.is_some());
    }

    #[test]
    fn test_fix_replace() {
        let fix = Fix::new(Span::new(5, 10), "replacement");

        assert_eq!(fix.span.start, 5);
        assert_eq!(fix.span.end, 10);
        assert_eq!(fix.text, "replacement");
    }

    #[test]
    fn test_fix_insert_at_beginning() {
        let fix = Fix::insert(0, "prefix");

        assert_eq!(fix.span.start, 0);
        assert_eq!(fix.span.end, 0);
        assert_eq!(fix.text, "prefix");
    }

    #[test]
    fn test_diagnostic_serialization() {
        let diag = Diagnostic::new("no-todo", "Found TODO", Span::new(10, 14));
        let json = serde_json::to_string(&diag).unwrap();

        assert!(json.contains("no-todo"));
        assert!(json.contains("Found TODO"));
    }

    #[test]
    fn test_diagnostic_deserialization() {
        let json = r#"{
            "rule_id": "no-todo",
            "message": "Found TODO",
            "span": { "start": 0, "end": 4 }
        }"#;

        let diag: Diagnostic = serde_json::from_str(json).unwrap();

        assert_eq!(diag.rule_id, "no-todo");
        assert_eq!(diag.message, "Found TODO");
        assert_eq!(diag.span.start, 0);
        assert_eq!(diag.span.end, 4);
    }

    #[test]
    fn test_fix_serialization() {
        let fix = Fix::new(Span::new(0, 5), "new text");
        let json = serde_json::to_string(&fix).unwrap();

        assert!(json.contains("new text"));
    }

    #[test]
    fn test_diagnostic_clone() {
        let original =
            Diagnostic::new("rule", "msg", Span::new(0, 5)).with_severity(Severity::Warning);

        let cloned = original.clone();

        assert_eq!(original.rule_id, cloned.rule_id);
        assert_eq!(original.message, cloned.message);
        assert_eq!(original.severity, cloned.severity);
    }

    #[test]
    fn test_all_severity_levels() {
        let error = Diagnostic::new("r", "m", Span::new(0, 1)).with_severity(Severity::Error);
        let warning = Diagnostic::new("r", "m", Span::new(0, 1)).with_severity(Severity::Warning);
        let info = Diagnostic::new("r", "m", Span::new(0, 1)).with_severity(Severity::Info);

        assert_eq!(error.severity, Severity::Error);
        assert_eq!(warning.severity, Severity::Warning);
        assert_eq!(info.severity, Severity::Info);
    }

    #[test]
    fn test_diagnostic_sorting_and_deduplication() {
        let diag1 = Diagnostic::new("rule1", "msg1", Span::new(10, 20));
        let diag2 = Diagnostic::new("rule1", "msg1", Span::new(10, 20)); // Exact duplicate of diag1
        let diag3 =
            Diagnostic::new("rule1", "msg1", Span::new(10, 20)).with_severity(Severity::Warning); // Different severity
        let diag4 = Diagnostic::new("rule1", "msg1", Span::new(5, 15)); // Earlier span
        let diag5 = Diagnostic::new("rule2", "msg1", Span::new(10, 20)); // Different rule

        let mut diagnostics = vec![
            diag1.clone(),
            diag2,
            diag3.clone(),
            diag4.clone(),
            diag5.clone(),
        ];

        // 1. Sort (using derived Ord)
        diagnostics.sort();

        // 2. Dedup
        diagnostics.dedup();

        // Should have 4 unique diagnostics (diag2 removed)
        assert_eq!(diagnostics.len(), 4);

        // Verify content
        assert!(diagnostics.contains(&diag1));
        assert!(diagnostics.contains(&diag3));
        assert!(diagnostics.contains(&diag4));
        assert!(diagnostics.contains(&diag5));

        // 3. Sort by span (simulating linter logic)
        diagnostics.sort_by(|a, b| a.span.start.cmp(&b.span.start));

        // diag4 should be first (starts at 5)
        assert_eq!(diagnostics[0], diag4);

        // The others start at 10, their relative order depends on other fields
        // But they should follow diag4
        assert_eq!(diagnostics[1].span.start, 10);
        assert_eq!(diagnostics[2].span.start, 10);
        assert_eq!(diagnostics[3].span.start, 10);
    }
}
