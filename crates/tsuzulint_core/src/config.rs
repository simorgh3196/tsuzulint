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

/// Maximum allowed size for configuration files (1 MB).
const MAX_CONFIG_SIZE: u64 = 1024 * 1024;

/// Configuration for the linter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinterConfig {
    /// Rules to load (plugins).
    #[serde(default)]
    pub rules: Vec<RuleDefinition>,

    /// Rule configuration (enable/disable/options).
    #[serde(default)]
    pub options: HashMap<String, RuleOption>,

    /// File patterns to include.
    #[serde(default)]
    pub include: Vec<String>,

    /// File patterns to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Cache settings.
    #[serde(default)]
    pub cache: CacheConfig,

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

fn default_cache_dir() -> &'static str {
    ".tsuzulint-cache"
}

/// Cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CacheConfig {
    /// Shorthand for enabling/disabling cache.
    Boolean(bool),
    /// Detailed cache configuration.
    Detail(CacheConfigDetail),
}

impl CacheConfig {
    /// Returns whether caching is enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            CacheConfig::Boolean(enabled) => *enabled,
            CacheConfig::Detail(detail) => detail.enabled,
        }
    }

    /// Returns the cache directory path.
    pub fn path(&self) -> &str {
        match self {
            CacheConfig::Boolean(_) => default_cache_dir(),
            CacheConfig::Detail(detail) => &detail.path,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self::Boolean(default_cache())
    }
}

/// Detailed cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheConfigDetail {
    /// Whether to enable caching.
    #[serde(default = "default_cache")]
    pub enabled: bool,
    /// Cache directory path.
    #[serde(default = "default_cache_dir_string")]
    pub path: String,
}

fn default_cache_dir_string() -> String {
    default_cache_dir().to_string()
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
    /// Supported configuration files in order of precedence.
    pub const CONFIG_FILES: &'static [&'static str] = &[".tsuzulint.jsonc", ".tsuzulint.json"];

    /// Attempts to find a configuration file in the given directory.
    pub fn discover(base_dir: impl AsRef<Path>) -> Option<PathBuf> {
        let base_dir = base_dir.as_ref();
        for name in Self::CONFIG_FILES {
            let path = base_dir.join(name);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            options: HashMap::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            cache: CacheConfig::default(),
            timings: false,
            base_dir: None,
        }
    }

    /// Loads configuration from a file.
    ///
    /// Supports `.tsuzulint.jsonc`, `.tsuzulint.json`.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, LinterError> {
        let path = path.as_ref();

        let metadata = fs::metadata(path).map_err(|e| {
            LinterError::config(format!(
                "Failed to read metadata for {}: {}",
                path.display(),
                e
            ))
        })?;

        if metadata.len() > MAX_CONFIG_SIZE {
            return Err(LinterError::config(format!(
                "Config file too large: {} bytes exceeds limit of {} bytes",
                metadata.len(),
                MAX_CONFIG_SIZE
            )));
        }

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
        // Use jsonc-parser to support comments
        let value =
            jsonc_parser::parse_to_serde_value(json, &jsonc_parser::ParseOptions::default())
                .map_err(|e| LinterError::config(format!("Invalid JSONC: {}", e)))?
                .unwrap_or(serde_json::Value::Null);

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
    pub fn hash(&self) -> Result<[u8; 32], LinterError> {
        let json = serde_json::to_string(self)
            .map_err(|e| LinterError::Internal(format!("Failed to serialize config: {}", e)))?;
        Ok(blake3::hash(json.as_bytes()).into())
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
        assert!(config.cache.is_enabled());
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
    fn test_config_from_jsonc() {
        let json = r#"{
            // This is a comment
            "options": {
                "no-todo": true, // Trailing comment
                /* Block comment */
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
        assert!(config.include.is_empty());
        assert!(config.exclude.is_empty());
        assert!(config.cache.is_enabled());
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

    #[test]
    fn test_config_cache_object() {
        let json = r#"{
            "cache": {
                "enabled": true,
                "path": ".custom-cache"
            }
        }"#;

        let config = LinterConfig::from_json(json).unwrap();
        assert!(config.cache.is_enabled());
        assert_eq!(config.cache.path(), ".custom-cache");
    }

    #[test]
    fn test_config_cache_object_path_only() {
        // enabled should default to true
        let json = r#"{ "cache": { "path": ".custom-cache" } }"#;
        let config = LinterConfig::from_json(json).unwrap();
        assert!(config.cache.is_enabled());
        assert_eq!(config.cache.path(), ".custom-cache");
    }

    #[test]
    fn test_config_cache_object_empty() {
        // Both fields should use their default values
        let json = r#"{ "cache": {} }"#;
        let config = LinterConfig::from_json(json).unwrap();
        assert!(config.cache.is_enabled());
        assert_eq!(config.cache.path(), ".tsuzulint-cache");
    }

    #[rstest]
    #[case::unknown_property(
        r#"{ "ruless": [] }"#,
        "Config validation failed" // Additional properties not allowed
    )]
    #[case::type_mismatch(
        r#"{ "cache": "not-a-string-or-bool" }"#,
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

    #[test]
    fn test_config_hash_consistency() {
        let config1 = LinterConfig::new();
        let config2 = LinterConfig::new();

        // Same configs should produce same hash
        assert_eq!(config1.hash().unwrap(), config2.hash().unwrap());
    }

    #[test]
    fn test_config_hash_changes_with_content() {
        let mut config1 = LinterConfig::new();
        let mut config2 = LinterConfig::new();

        config2.cache = CacheConfig::Boolean(false);

        // Different configs should produce different hashes
        assert_ne!(config1.hash().unwrap(), config2.hash().unwrap());

        // Adding rules should change hash
        config1
            .rules
            .push(RuleDefinition::Simple("test-rule".to_string()));
        let hash_after_rule = config1.hash().unwrap();
        assert_ne!(LinterConfig::new().hash().unwrap(), hash_after_rule);
    }

    #[test]
    fn test_config_from_file_sets_base_dir() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(".tsuzulint.json");
        fs::write(&config_path, r#"{"cache": true}"#).unwrap();

        let config = LinterConfig::from_file(&config_path).unwrap();
        assert_eq!(config.base_dir, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_config_from_file_handles_root_directory() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(".tsuzulint.json");
        fs::write(&config_path, r#"{}"#).unwrap();

        let config = LinterConfig::from_file(&config_path).unwrap();
        assert!(config.base_dir.is_some());
    }

    #[test]
    fn test_config_from_file_not_found() {
        let result = LinterConfig::from_file("non_existent_file_xyz.json");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to read config")
                || err_msg.contains("Failed to read metadata"),
            "Error message '{}' does not contain expected text",
            err_msg
        );
    }

    #[test]
    fn test_config_from_file_invalid_json() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("invalid.json");
        fs::write(&config_path, "{ invalid json }").unwrap();

        let result = LinterConfig::from_file(&config_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSONC"));
    }

    #[test]
    fn test_config_from_file_jsonc_support() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("config.jsonc");
        let content = r#"{
            // Comment
            "cache": false
        }"#;
        fs::write(&config_path, content).unwrap();

        let config = LinterConfig::from_file(&config_path).unwrap();
        assert!(!config.cache.is_enabled());
    }

    #[test]
    fn test_enabled_rules_excludes_disabled() {
        let json = r#"{
            "options": {
                "enabled": true,
                "disabled": false,
                "off": "off"
            }
        }"#;

        let config = LinterConfig::from_json(json).unwrap();
        let enabled = config.enabled_rules();

        assert_eq!(enabled.len(), 1);
        assert!(enabled.iter().any(|(name, _)| *name == "enabled"));
        assert!(!enabled.iter().any(|(name, _)| *name == "disabled"));
        assert!(!enabled.iter().any(|(name, _)| *name == "off"));
    }

    #[test]
    fn test_rule_definition_detail_equality() {
        let def1 = RuleDefinitionDetail {
            github: Some("owner/repo".to_string()),
            url: None,
            path: None,
            r#as: Some("alias".to_string()),
        };

        let def2 = RuleDefinitionDetail {
            github: Some("owner/repo".to_string()),
            url: None,
            path: None,
            r#as: Some("alias".to_string()),
        };

        let def3 = RuleDefinitionDetail {
            github: Some("other/repo".to_string()),
            url: None,
            path: None,
            r#as: Some("alias".to_string()),
        };

        assert_eq!(def1, def2);
        assert_ne!(def1, def3);
    }

    #[test]
    fn test_rule_option_severity_variants() {
        let error = RuleOption::Severity("error".to_string());
        let warning = RuleOption::Severity("warning".to_string());
        let off = RuleOption::Severity("off".to_string());

        assert!(error.is_enabled());
        assert!(warning.is_enabled());
        assert!(!off.is_enabled());
    }

    #[test]
    fn test_config_from_json_with_null_value() {
        let json = r#"null"#;

        // Should handle null as empty config or error
        let result = LinterConfig::from_json(json);
        // null is not a valid object, so it should fail validation
        assert!(result.is_err());
    }

    #[test]
    fn test_config_cache_dir_default() {
        let config = LinterConfig::new();
        assert_eq!(config.cache.path(), ".tsuzulint-cache");
    }

    #[test]
    fn test_config_include_exclude_empty_by_default() {
        let config = LinterConfig::new();
        assert!(config.include.is_empty());
        assert!(config.exclude.is_empty());
    }

    #[test]
    fn test_config_with_complex_rule_definitions() {
        let json = r#"{
            "rules": [
                "simple/rule",
                { "github": "owner/repo@v1.0.0", "as": "custom-name" },
                { "url": "https://example.com/rule.wasm", "as": "url-rule" },
                { "path": "./local/rule.wasm", "as": "local-rule" }
            ]
        }"#;

        let config = LinterConfig::from_json(json).unwrap();
        assert_eq!(config.rules.len(), 4);

        // Verify first is Simple
        match &config.rules[0] {
            RuleDefinition::Simple(s) => assert_eq!(s, "simple/rule"),
            _ => panic!("Expected Simple"),
        }

        // Verify second is Detail with github
        match &config.rules[1] {
            RuleDefinition::Detail(d) => {
                assert_eq!(d.github.as_deref(), Some("owner/repo@v1.0.0"));
                assert_eq!(d.r#as.as_deref(), Some("custom-name"));
            }
            _ => panic!("Expected Detail"),
        }
    }

    #[test]
    fn test_config_from_file_too_large() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("large_config.json");

        // Create a file slightly larger than MAX_CONFIG_SIZE
        let size = MAX_CONFIG_SIZE as usize + 1;
        let content = " ".repeat(size); // Valid JSON whitespace
        fs::write(&config_path, content).unwrap();

        let result = LinterConfig::from_file(&config_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Config file too large")
        );
    }
}
