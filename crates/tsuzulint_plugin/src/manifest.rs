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

/// Supported languages for text analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KnownLanguage {
    #[default]
    Ja, // Japanese
    En, // English
    Zh, // Chinese (future)
    Ko, // Korean (future)
}

/// Required analysis capabilities for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Morphology, // 形態素解析 (tokens)
    Sentences,  // 文分割
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
            isolation_level: IsolationLevel::Global,
            schema: None,
            languages: Vec::new(),
            capabilities: Vec::new(),
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

    /// Returns true if this rule requires morphology analysis.
    pub fn needs_morphology(&self) -> bool {
        self.capabilities.contains(&Capability::Morphology)
    }

    /// Returns true if this rule requires sentence splitting.
    pub fn needs_sentences(&self) -> bool {
        self.capabilities.contains(&Capability::Sentences)
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
        assert!(manifest.languages.is_empty());
        assert!(manifest.capabilities.is_empty());
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = RuleManifest::new("no-todo", "1.0.0")
            .with_description("Disallow TODO comments")
            .with_fixable(true)
            .with_node_types(vec!["Str".to_string()])
            .with_isolation_level(IsolationLevel::Block)
            .with_languages(vec![KnownLanguage::Ja])
            .with_capabilities(vec![Capability::Morphology]);

        assert_eq!(
            manifest.description,
            Some("Disallow TODO comments".to_string())
        );
        assert!(manifest.fixable);
        assert_eq!(manifest.node_types, vec!["Str"]);
        assert_eq!(manifest.isolation_level, IsolationLevel::Block);
        assert_eq!(manifest.languages, vec![KnownLanguage::Ja]);
        assert_eq!(manifest.capabilities, vec![Capability::Morphology]);
    }

    #[test]
    fn test_manifest_needs_morphology() {
        let manifest =
            RuleManifest::new("test", "1.0.0").with_capabilities(vec![Capability::Morphology]);
        assert!(manifest.needs_morphology());
        assert!(!manifest.needs_sentences());

        let manifest_no_caps = RuleManifest::new("test", "1.0.0");
        assert!(!manifest_no_caps.needs_morphology());
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
    fn test_manifest_serialization_with_capabilities() {
        let manifest = RuleManifest::new("test-rule", "0.1.0")
            .with_languages(vec![KnownLanguage::Ja])
            .with_capabilities(vec![Capability::Morphology, Capability::Sentences]);
        let json = serde_json::to_string(&manifest).unwrap();

        assert!(json.contains("\"languages\":[\"ja\"]"));
        assert!(json.contains("\"capabilities\":[\"morphology\",\"sentences\"]"));
    }

    #[test]
    fn test_manifest_deserialization_default_isolation() {
        let json = r#"{
            "name": "test-rule",
            "version": "0.1.0"
        }"#;

        let manifest: RuleManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.isolation_level, IsolationLevel::Global);
        assert!(manifest.languages.is_empty());
        assert!(manifest.capabilities.is_empty());
    }

    #[test]
    fn test_manifest_deserialization_with_capabilities() {
        let json = r#"{
            "name": "test-rule",
            "version": "0.1.0",
            "languages": ["ja"],
            "capabilities": ["morphology"]
        }"#;

        let manifest: RuleManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.languages, vec![KnownLanguage::Ja]);
        assert_eq!(manifest.capabilities, vec![Capability::Morphology]);
        assert!(manifest.needs_morphology());
    }

    #[test]
    fn test_known_language_serialization() {
        assert_eq!(serde_json::to_string(&KnownLanguage::Ja).unwrap(), "\"ja\"");
        assert_eq!(serde_json::to_string(&KnownLanguage::En).unwrap(), "\"en\"");
    }

    #[test]
    fn test_capability_serialization() {
        assert_eq!(
            serde_json::to_string(&Capability::Morphology).unwrap(),
            "\"morphology\""
        );
        assert_eq!(
            serde_json::to_string(&Capability::Sentences).unwrap(),
            "\"sentences\""
        );
    }
}
