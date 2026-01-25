//! Rule manifest definition.

use serde::{Deserialize, Serialize};

/// Isolation level for rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IsolationLevel {
    /// Rule requires the entire document context.
    #[default]
    Global,
    /// Rule can be run on individual blocks (e.g., paragraphs) independently.
    Block,
}

/// Manifest for a WASM rule plugin.
///
/// Every rule must export a `get_manifest` function that returns
/// this structure serialized as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleManifest {
    /// Unique rule identifier (e.g., "no-todo").
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
    ///
    /// If empty, the rule will receive all nodes.
    #[serde(default)]
    pub node_types: Vec<String>,

    /// Isolation level for this rule.
    #[serde(default)]
    pub isolation_level: IsolationLevel,

    /// JSON Schema for rule options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
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
            isolation_level: IsolationLevel::Global,
            schema: None,
        }
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets whether the rule is fixable.
    pub fn with_fixable(mut self, fixable: bool) -> Self {
        self.fixable = fixable;
        self
    }

    /// Sets the node types this rule handles.
    pub fn with_node_types(mut self, node_types: Vec<String>) -> Self {
        self.node_types = node_types;
        self
    }

    /// Sets the isolation level for this rule.
    pub fn with_isolation_level(mut self, isolation_level: IsolationLevel) -> Self {
        self.isolation_level = isolation_level;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_new() {
        let manifest = RuleManifest::new("no-todo", "1.0.0");

        assert_eq!(manifest.name, "no-todo");
        assert_eq!(manifest.version, "1.0.0");
        assert!(!manifest.fixable);
        assert_eq!(manifest.isolation_level, IsolationLevel::Global);
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = RuleManifest::new("no-todo", "1.0.0")
            .with_description("Disallow TODO comments")
            .with_fixable(true)
            .with_node_types(vec!["Str".to_string()])
            .with_isolation_level(IsolationLevel::Block);

        assert_eq!(
            manifest.description,
            Some("Disallow TODO comments".to_string())
        );
        assert!(manifest.fixable);
        assert_eq!(manifest.node_types, vec!["Str"]);
        assert_eq!(manifest.isolation_level, IsolationLevel::Block);
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = RuleManifest::new("test-rule", "0.1.0");
        let json = serde_json::to_string(&manifest).unwrap();

        assert!(json.contains("\"name\":\"test-rule\""));
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"isolation_level\":\"global\""));
    }

    #[test]
    fn test_manifest_deserialization_default_isolation() {
        let json = r#"{
            "name": "test-rule",
            "version": "0.1.0"
        }"#;

        let manifest: RuleManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.isolation_level, IsolationLevel::Global);
    }
}
