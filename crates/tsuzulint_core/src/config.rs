//! Linter configuration.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::LinterError;

use jsonschema::Validator;
use std::sync::OnceLock;

// Embed the schema
const SCHEMA_JSON: &str = include_str!("../../../schemas/v1/config.json");
static CONFIG_SCHEMA: OnceLock<Validator> = OnceLock::new();

/// Configuration for the linter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinterConfig {
    /// Rules to load (plugins).
    #[serde(default)]
    pub rules: Vec<RuleDefinition>,

    /// Rule configuration (enable/disable/options).
    #[serde(default)]
    pub options: HashMap<String, RuleOption>,

    /// Plugin names to load (for backward compatibility or simpler plugin lists).
    #[serde(default)]
    pub plugins: Vec<String>,

    /// File patterns to include.
    #[serde(default)]
    pub include: Vec<String>,

    /// File patterns to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Whether to enable caching.
    #[serde(default = "default_cache")]
    pub cache: bool,

    /// Cache directory.
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,

    /// Whether to enable performance timings.
    #[serde(default)]
    pub timings: bool,

    /// Base directory for resolving relative paths (plugins, etc.).
    /// This is usually the directory containing the configuration file.
    #[serde(skip)]
    pub base_dir: Option<PathBuf>,
}

fn default_cache() -> bool {
    true
}

fn default_cache_dir() -> String {
    ".tsuzulint-cache".to_string()
}

/// Definition of a rule to load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RuleDefinition {
    /// String shorthand: "owner/repo" or "owner/repo@version"
    Simple(String),
    /// Detailed definition object
    Detail(RuleDefinitionDetail),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleDefinitionDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#as: Option<String>,
}

/// Configuration for a single rule (in options map).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RuleOption {
    /// Rule is enabled/disabled (boolean).
    Enabled(bool),
    /// Rule is enabled with severity string ("error", "warning", "off").
    Severity(String),
    /// Rule is enabled with specific options object.
    Options(serde_json::Value),
}

impl RuleOption {
    /// Returns whether the rule is enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            RuleOption::Enabled(enabled) => *enabled,
            RuleOption::Severity(s) => s != "off",
            RuleOption::Options(_) => true,
        }
    }

    /// Gets the rule options as JSON value.
    pub fn options(&self) -> serde_json::Value {
        match self {
            RuleOption::Enabled(_) => serde_json::Value::Null,
            RuleOption::Severity(_) => serde_json::Value::Null,
            RuleOption::Options(v) => v.clone(),
        }
    }
}

impl LinterConfig {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            options: HashMap::new(),
            plugins: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            cache: true,
            cache_dir: default_cache_dir(),
            timings: false,
            base_dir: None,
        }
    }

    /// Loads configuration from a file.
    ///
    /// Supports `.tsuzulint.jsonc`, `.tsuzulint.json`.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, LinterError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .map_err(|e| LinterError::config(format!("Failed to read config: {}", e)))?;

        let mut config = Self::from_json(&content)?;

        // precise parent directory handling
        if let Some(parent) = path.parent() {
            config.base_dir = Some(parent.to_path_buf());
        }

        Ok(config)
    }

    /// Parses configuration from JSON string with schema validation.
    pub fn from_json(json: &str) -> Result<Self, LinterError> {
        // Parse into Value first for validation
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| LinterError::config(format!("Invalid JSON: {}", e)))?;

        // Initialize and check schema
        let schema = CONFIG_SCHEMA.get_or_init(|| {
            let schema_json: serde_json::Value =
                serde_json::from_str(SCHEMA_JSON).expect("Invalid embedded config schema");
            Validator::new(&schema_json).expect("Invalid config schema compilation")
        });

        if let Err(e) = schema.validate(&value) {
            let error_msg = format!("{} at {}", e, e.instance_path());
            return Err(LinterError::config(format!(
                "Config validation failed: {}",
                error_msg
            )));
        }

        serde_json::from_value(value)
            .map_err(|e| LinterError::config(format!("Invalid config: {}", e)))
    }

    /// Returns enabled rules (Iterator over options).
    /// Note: This only lists rules present in the `options` map.
    /// Rules loaded via `rules` array but not configured in `options` are NOT included here.
    pub fn enabled_rules(&self) -> Vec<(&str, &RuleOption)> {
        self.options
            .iter()
            .filter(|(_, config)| config.is_enabled())
            .map(|(name, config)| (name.as_str(), config))
            .collect()
    }

    /// Computes a hash of the configuration for cache invalidation.
    pub fn hash(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        blake3::hash(json.as_bytes()).to_hex().to_string()
    }
}

impl Default for LinterConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = LinterConfig::new();
        assert!(config.rules.is_empty());
        assert!(config.options.is_empty());
        assert!(config.cache);
    }

    #[test]
    fn test_config_from_json() {
        let json = r#"{
            "options": {
                "no-todo": true,
                "max-lines": { "max": 100 }
            }
        }"#;

        let config = LinterConfig::from_json(json).unwrap();
        assert_eq!(config.options.len(), 2);
    }

    #[test]
    fn test_config_rules_array() {
        let json = r#"{
            "rules": [
                "simorgh3196/tsuzulint-rule-no-doubled-joshi",
                { "github": "alice/foo", "as": "foo" }
            ],
            "options": {
                "no-doubled-joshi": true
            }
        }"#;

        let config = LinterConfig::from_json(json).unwrap();
        assert_eq!(config.rules.len(), 2);

        match &config.rules[0] {
            RuleDefinition::Simple(s) => {
                assert_eq!(s, "simorgh3196/tsuzulint-rule-no-doubled-joshi")
            }
            _ => panic!("Expected Simple rule definition"),
        }

        match &config.rules[1] {
            RuleDefinition::Detail(d) => {
                assert_eq!(d.github.as_deref(), Some("alice/foo"));
                assert_eq!(d.r#as.as_deref(), Some("foo"));
            }
            _ => panic!("Expected Detail rule definition"),
        }
    }

    #[test]
    fn test_rule_option_enabled() {
        let enabled = RuleOption::Enabled(true);
        let disabled = RuleOption::Enabled(false);
        let off = RuleOption::Severity("off".to_string());
        let error = RuleOption::Severity("error".to_string());

        assert!(enabled.is_enabled());
        assert!(!disabled.is_enabled());
        assert!(!off.is_enabled());
        assert!(error.is_enabled());
    }

    #[test]
    fn test_enabled_rules() {
        let json = r#"{
            "options": {
                "enabled-rule": true,
                "disabled-rule": false,
                "options-rule": { "option": "value" }
            }
        }"#;

        let config = LinterConfig::from_json(json).unwrap();
        let enabled = config.enabled_rules();

        assert_eq!(enabled.len(), 2); // enabled-rule, options-rule
    }

    #[test]
    fn test_config_default() {
        let config = LinterConfig::default();
        assert!(config.rules.is_empty());
        assert!(config.options.is_empty());
        assert!(config.plugins.is_empty());
        assert!(config.include.is_empty());
        assert!(config.exclude.is_empty());
        assert!(config.cache);
    }

    #[test]
    fn test_rule_option_values() {
        let options = RuleOption::Options(serde_json::json!({"max": 100}));
        let enabled = RuleOption::Enabled(true);
        let severity = RuleOption::Severity("error".to_string());

        assert!(options.is_enabled());
        let opts = options.options();
        assert_eq!(opts["max"], 100);

        assert_eq!(enabled.options(), serde_json::Value::Null);
        assert_eq!(severity.options(), serde_json::Value::Null);
    }

    use rstest::rstest;

    #[rstest]
    #[case::unknown_property(
        r#"{ "ruless": [] }"#,
        "Config validation failed" // Additional properties not allowed
    )]
    #[case::type_mismatch(
        r#"{ "cache": "not-a-bool" }"#,
        "Config validation failed" // Type mismatch
    )]
    #[case::invalid_enum_value(
        r#"{ "options": { "rule-id": "invalid-severity" } }"#,
        "Config validation failed" // Enum validation
    )]
    fn test_config_validation_errors(#[case] json: &str, #[case] expected_error_part: &str) {
        let result = LinterConfig::from_json(json);
        assert!(result.is_err(), "Expected error for JSON: {}", json);
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains(expected_error_part),
            "Error message '{}' should contain '{}'",
            err,
            expected_error_part
        );
    }
}
