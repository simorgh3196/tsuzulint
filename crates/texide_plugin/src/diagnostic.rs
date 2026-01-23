//! Diagnostic types for lint results.

use serde::{Deserialize, Serialize};
use texide_ast::{Location, Span};

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
