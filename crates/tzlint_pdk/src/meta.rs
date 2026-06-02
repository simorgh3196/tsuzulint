//! Rule metadata the engine uses to schedule a rule.

use alloc::vec::Vec;

use tzlint_ast::NodeKind;
use tzlint_ast::morphology::Lang;

use crate::{RuleId, Severity};

/// What a rule needs from the engine before it can run, beyond visiting its [`NodeKind`]s.
///
/// This is **additive and defaults to "nothing"** ([`Requirements::default`]): an existing rule
/// declares no requirements and is scheduled exactly as before. A rule that reads morphological
/// tokens opts in via [`RuleMeta::with_morphology`], which lets the engine provision the right
/// dictionary and skip the rule (rather than feed it an empty table) when morphology is
/// unavailable for the document.
///
/// The fields are **private and only ever set together**, so an inconsistent state — a pinned
/// language without a morphology requirement — is unrepresentable: the engine can trust that
/// [`lang`](Self::lang) is `Some` exactly when [`needs_morphology`](Self::needs_morphology) is
/// `true`. The shape is kept open for a future language-agnostic morphology rule
/// (morphology required, no pinned language); that would be added through a new builder, never by
/// constructing this struct directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Requirements {
    /// Whether the rule reads the morphology table for the nodes it visits.
    morphology: bool,
    /// The dictionary language the morphology backend must provision for this rule, if pinned.
    /// Set together with `morphology`, so it is `Some` only when `morphology` is `true`.
    lang: Option<Lang>,
}

impl Requirements {
    /// Whether the rule reads the morphology table for the nodes it visits.
    #[must_use]
    pub fn needs_morphology(&self) -> bool {
        self.morphology
    }

    /// The dictionary language the morphology backend must provision for this rule. `Some` only
    /// when [`needs_morphology`](Self::needs_morphology) is `true`; the two are set together.
    #[must_use]
    pub fn lang(&self) -> Option<Lang> {
        self.lang
    }
}

/// Static metadata describing a rule: its id, the [`NodeKind`]s it wants to visit, its default
/// severity, and what it [`requires`](RuleMeta::requires) to run.
///
/// The engine applies the `node_kinds` filter itself (a rule is invoked only at matching
/// nodes), so a rule cannot silently observe nothing because it forgot to filter. A
/// cross-node (document-level) rule self-traverses via `NodeRef`: it either registers for
/// [`NodeKind::ROOT`] and walks the subtree from its [`check`](crate::Rule::check), or
/// registers for no kind and walks from [`finish`](crate::Rule::finish) via
/// [`Context::ast`](crate::Context::ast).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMeta {
    pub id: RuleId,
    pub node_kinds: Vec<NodeKind>,
    pub default_severity: Severity,
    /// Capabilities the rule needs before it runs. Defaults to [`Requirements::default`]
    /// (nothing) — see [`with_morphology`](RuleMeta::with_morphology) to opt in. Private so the
    /// pair stays consistent; read it via [`requires`](RuleMeta::requires).
    requires: Requirements,
}

impl RuleMeta {
    /// Build metadata for a rule. It requires nothing beyond visiting its `node_kinds`; opt into
    /// extra capabilities with the builder methods (e.g. [`with_morphology`](Self::with_morphology)).
    pub fn new(
        id: impl Into<RuleId>,
        default_severity: Severity,
        node_kinds: impl Into<Vec<NodeKind>>,
    ) -> Self {
        RuleMeta {
            id: id.into(),
            default_severity,
            node_kinds: node_kinds.into(),
            requires: Requirements::default(),
        }
    }

    /// Declare that this rule reads the morphology table, pinned to dictionary language `lang`.
    ///
    /// The engine uses this to provision `lang`'s dictionary and to skip the rule when
    /// morphology is unavailable, instead of running it against an empty table.
    #[must_use]
    pub fn with_morphology(mut self, lang: Lang) -> Self {
        self.requires = Requirements {
            morphology: true,
            lang: Some(lang),
        };
        self
    }

    /// Whether this rule wants to visit `kind`.
    pub fn visits(&self, kind: NodeKind) -> bool {
        self.node_kinds.contains(&kind)
    }

    /// The capabilities this rule needs before it runs (see [`with_morphology`](Self::with_morphology)).
    #[must_use]
    pub fn requires(&self) -> Requirements {
        self.requires
    }

    /// Whether this rule reads the morphology table (see [`with_morphology`](Self::with_morphology)).
    #[must_use]
    pub fn needs_morphology(&self) -> bool {
        self.requires.needs_morphology()
    }

    /// The dictionary language this rule's morphology requirement is pinned to, if any.
    #[must_use]
    pub fn required_lang(&self) -> Option<Lang> {
        self.requires.lang()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use tzlint_ast::morphology::Lang;

    #[test]
    fn visits_only_registered_kinds() {
        let meta = RuleMeta::new(
            "sentence-length",
            Severity::Warning,
            vec![NodeKind::PARAGRAPH, NodeKind::HEADING],
        );
        assert!(meta.visits(NodeKind::PARAGRAPH));
        assert!(meta.visits(NodeKind::HEADING));
        assert!(!meta.visits(NodeKind::TEXT));
        assert_eq!(meta.default_severity, Severity::Warning);
        assert_eq!(meta.id.as_str(), "sentence-length");
    }

    #[test]
    fn new_meta_requires_no_morphology_by_default() {
        let meta = RuleMeta::new("plain", Severity::Warning, vec![NodeKind::TEXT]);
        assert!(!meta.needs_morphology());
        assert_eq!(meta.required_lang(), None);
        assert_eq!(meta.requires(), Requirements::default());
    }

    #[test]
    fn requires_exposes_the_capability_bundle_through_accessors() {
        let plain = RuleMeta::new("plain", Severity::Warning, vec![NodeKind::TEXT]);
        let bundle = plain.requires();
        assert!(!bundle.needs_morphology());
        assert_eq!(bundle.lang(), None);

        let morph = RuleMeta::new("no-doubled-joshi", Severity::Warning, vec![NodeKind::TEXT])
            .with_morphology(Lang::JA);
        let bundle = morph.requires();
        assert!(bundle.needs_morphology());
        assert_eq!(bundle.lang(), Some(Lang::JA));
    }

    #[test]
    fn with_morphology_records_the_requirement_and_language() {
        let meta = RuleMeta::new("no-doubled-joshi", Severity::Warning, vec![NodeKind::TEXT])
            .with_morphology(Lang::JA);
        assert!(meta.needs_morphology());
        assert_eq!(meta.required_lang(), Some(Lang::JA));
    }

    #[test]
    fn with_morphology_leaves_the_other_metadata_intact() {
        let meta = RuleMeta::new("no-doubled-joshi", Severity::Error, vec![NodeKind::TEXT])
            .with_morphology(Lang::JA);
        assert_eq!(meta.id.as_str(), "no-doubled-joshi");
        assert_eq!(meta.default_severity, Severity::Error);
        assert!(meta.visits(NodeKind::TEXT));
    }
}
