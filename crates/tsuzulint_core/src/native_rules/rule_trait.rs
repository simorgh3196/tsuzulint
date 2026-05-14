//! The `Rule` trait that every native rule implements.

use tsuzulint_plugin::Diagnostic;

use super::context::RuleContext;

/// A rule that runs natively inside the linter binary.
///
/// Implementations should be cheap to construct (the registry holds one
/// instance per rule for the process lifetime) and must be `Send + Sync`
/// because the linter dispatches files across a rayon thread pool.
///
/// The `lint` method is expected to walk the AST it cares about and return
/// diagnostics. Rules are responsible for their own descendant traversal —
/// the linter hands over the whole document and lets the rule decide what
/// nodes are interesting (this matches how ESLint / textlint rules work).
pub trait Rule: Send + Sync {
    /// Stable identifier used in configs and diagnostics (e.g. `"no-todo"`).
    fn name(&self) -> &'static str;

    /// Human-readable description used in docs and error messages.
    fn description(&self) -> &'static str {
        ""
    }

    /// Whether the rule can produce auto-fix suggestions.
    fn fixable(&self) -> bool {
        false
    }

    /// Whether the rule needs morphological tokens to run.
    ///
    /// The linter skips the tokenizer entirely when no enabled rule declares
    /// it needs morphology, so flipping this from `false` to `true` has a
    /// real cost on languages that require morphological analysis.
    fn needs_morphology(&self) -> bool {
        false
    }

    /// Whether the rule needs pre-split sentences.
    fn needs_sentences(&self) -> bool {
        false
    }

    /// Run the rule and return any diagnostics.
    fn lint(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic>;
}
