//! Rule metadata the engine uses to schedule a rule.

use alloc::vec::Vec;

use tzlint_ast::NodeKind;

use crate::{RuleId, Severity};

/// Static metadata describing a rule: its id, the [`NodeKind`]s it wants to visit, and its
/// default severity.
///
/// The engine applies the `node_kinds` filter itself (a rule is invoked only at matching
/// nodes), so a rule cannot silently observe nothing because it forgot to filter. A
/// cross-node rule registers for [`NodeKind::ROOT`] and walks its subtree via `NodeRef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMeta {
    pub id: RuleId,
    pub node_kinds: Vec<NodeKind>,
    pub default_severity: Severity,
}

impl RuleMeta {
    /// Build metadata for a rule.
    pub fn new(
        id: impl Into<RuleId>,
        default_severity: Severity,
        node_kinds: impl Into<Vec<NodeKind>>,
    ) -> Self {
        RuleMeta {
            id: id.into(),
            default_severity,
            node_kinds: node_kinds.into(),
        }
    }

    /// Whether this rule wants to visit `kind`.
    pub fn visits(&self, kind: NodeKind) -> bool {
        self.node_kinds.contains(&kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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
}
