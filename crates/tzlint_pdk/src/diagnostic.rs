//! The diagnostic model rules emit and the engine aggregates.

use alloc::string::String;
use alloc::vec::Vec;

use tzlint_ast::Span;

/// A rule identifier.
///
/// Lowercase kebab-case; native rules use a bare id (`sentence-length`), plugin rules are
/// namespaced as `<namespace>/<rule>` (`acme/no-weasel-words`). Ids are a public contract —
/// greppable, stable, and used verbatim in config keys and the cache key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleId(String);

impl RuleId {
    /// Wrap an id string.
    pub fn new(id: impl Into<String>) -> Self {
        RuleId(id.into())
    }
    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for RuleId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for RuleId {
    fn from(s: &str) -> Self {
        RuleId(s.into())
    }
}

impl From<String> for RuleId {
    fn from(s: String) -> Self {
        RuleId(s)
    }
}

/// Diagnostic severity, declared most-to-least severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A suggested edit: replace the bytes at `span` (absolute into `Ast.text`) with
/// `replacement`. A pure deletion uses an empty `replacement`; a pure insertion uses an
/// empty `span`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
}

impl Fix {
    /// Replace `span` with `replacement`.
    pub fn replace(span: Span, replacement: impl Into<String>) -> Self {
        Fix {
            span,
            replacement: replacement.into(),
        }
    }
    /// Delete the bytes at `span`.
    pub fn delete(span: Span) -> Self {
        Fix {
            span,
            replacement: String::new(),
        }
    }
}

/// One reported problem: attributed to a rule, located at an absolute byte [`Span`], with
/// zero or more suggested [`Fix`]es.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub rule_id: RuleId,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    /// A diagnostic with no fixes.
    pub fn new(
        rule_id: impl Into<RuleId>,
        severity: Severity,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Diagnostic {
            rule_id: rule_id.into(),
            severity,
            message: message.into(),
            span,
            fixes: Vec::new(),
        }
    }

    /// Attach a [`Fix`] (builder style).
    #[must_use]
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// The stable within-file ordering key: `(span.start, span.end, rule_id, message)`.
    /// Output is sorted by `(file_path, …this key)` so it is independent of scheduling.
    pub fn sort_key(&self) -> (u32, u32, &str, &str) {
        (
            self.span.start,
            self.span.end,
            self.rule_id.as_str(),
            self.message.as_str(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_id_roundtrips() {
        let id = RuleId::from("sentence-length");
        assert_eq!(id.as_str(), "sentence-length");
        assert_eq!(id.to_string(), "sentence-length");
        assert_eq!(RuleId::new(String::from("acme/x")).as_str(), "acme/x");
        // From<String> (owned) as well as From<&str> above.
        assert_eq!(RuleId::from(String::from("acme/y")).as_str(), "acme/y");
    }

    #[test]
    fn severity_orders_most_severe_first() {
        assert!(Severity::Error < Severity::Warning);
        assert!(Severity::Warning < Severity::Info);
        assert!(Severity::Info < Severity::Hint);
    }

    #[test]
    fn fix_constructors() {
        assert_eq!(Fix::delete(Span::new(1, 4)).replacement, "");
        let f = Fix::replace(Span::new(1, 4), "x");
        assert_eq!(f.span, Span::new(1, 4));
        assert_eq!(f.replacement, "x");
    }

    #[test]
    fn diagnostic_builder_and_sort_key() {
        let d = Diagnostic::new(
            "max-comma",
            Severity::Warning,
            Span::new(5, 9),
            "too many commas",
        )
        .with_fix(Fix::delete(Span::new(8, 9)));
        assert_eq!(d.fixes.len(), 1);
        assert_eq!(d.sort_key(), (5, 9, "max-comma", "too many commas"));
    }
}
