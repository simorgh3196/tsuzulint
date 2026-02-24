use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use thiserror::Error;

/// Maximum length for rule names
pub const MAX_RULE_NAME_LENGTH: usize = 64;

/// Error type for manifest operations.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("Failed to parse manifest JSON: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("Manifest validation failed: {0}")]
    ValidationError(String),
}

/// The structure of `tsuzulint-rule.json`.
/// This matches `schemas/v1/rule.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalRuleManifest {
    pub rule: RuleMetadata,
    pub artifacts: Artifacts,
    #[serde(default)]
    pub permissions: Option<Permissions>,
    #[serde(default)]
    pub tsuzulint: Option<TsuzuLintCompatibility>,
    #[serde(default)]
    pub options: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub fixable: bool,
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default = "default_isolation_level")]
    pub isolation_level: IsolationLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IsolationLevel {
    #[default]
    Global,
    Block,
}

fn default_isolation_level() -> IsolationLevel {
    IsolationLevel::Global
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifacts {
    pub wasm: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub filesystem: Vec<FilesystemPermission>,
    #[serde(default)]
    pub network: Vec<NetworkPermission>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemPermission {
    pub path: String,
    pub access: FilesystemAccess,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilesystemAccess {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkPermission {
    pub host: String,
    pub access: NetworkAccess,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkAccess {
    Http,
    Https,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsuzuLintCompatibility {
    pub min_version: Option<String>,
}

// Embed the schema
// Path is relative to this file: ../../../schemas/v1/rule.json
const RULE_SCHEMA_JSON: &str = include_str!("../../../schemas/v1/rule.json");

static SCHEMA: OnceLock<Validator> = OnceLock::new();

/// Validates a rule name against the schema requirements.
///
/// A valid rule name:
/// - Is 1-64 characters long
/// - Contains no whitespace characters
/// - Contains no ASCII control characters (0x00-0x1F, 0x7F-0x9F)
///
/// This allows multilingual names (Japanese, Chinese, Korean, etc.) while
/// preventing problematic characters.
pub fn is_valid_rule_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_RULE_NAME_LENGTH {
        return false;
    }

    !name.chars().any(|c| {
        c.is_whitespace() || (c as u32) <= 0x1F || ((c as u32) >= 0x7F && (c as u32) <= 0x9F)
    })
}

/// Validates a manifest JSON string against the schema.
pub fn validate_manifest(json_str: &str) -> Result<ExternalRuleManifest, ManifestError> {
    // 1. Parse JSON to Value
    let instance: Value = serde_json::from_str(json_str)?;

    // 2. Validate against Schema
    let schema = SCHEMA.get_or_init(|| {
        let schema_json: Value =
            serde_json::from_str(RULE_SCHEMA_JSON).expect("Invalid embedded schema");
        Validator::new(&schema_json).expect("Invalid schema compilation")
    });

    if let Err(e) = schema.validate(&instance) {
        let error_msg = format!("{} at {}", e, e.instance_path());
        return Err(ManifestError::ValidationError(error_msg));
    }

    // 3. Deserialize to struct
    let manifest: ExternalRuleManifest = serde_json::from_value(instance)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_manifest() {
        let json = r#"{
            "rule": {
                "name": "no-todo",
                "version": "1.0.0",
                "description": "Disallow TODO",
                "fixable": false
            },
            "artifacts": {
                "wasm": "https://example.com/rule.wasm",
                "sha256": "a3b6408225010668045610815132640108602685710662650426543168015505"
            }
        }"#;

        let manifest = validate_manifest(json).expect("Validation should pass");
        assert_eq!(manifest.rule.name, "no-todo");
    }

    #[test]
    fn test_valid_manifest_multilingual() {
        let json = r#"{
            "rule": {
                "name": "敬語チェック",
                "version": "1.0.0",
                "description": "Check keigo usage"
            },
            "artifacts": {
                "wasm": "https://example.com/rule.wasm",
                "sha256": "a3b6408225010668045610815132640108602685710662650426543168015505"
            }
        }"#;

        let manifest =
            validate_manifest(json).expect("Validation should pass for multilingual name");
        assert_eq!(manifest.rule.name, "敬語チェック");
    }

    #[test]
    fn test_invalid_manifest_missing_field() {
        let json = r#"{
            "rule": {
                "name": "no-todo"
            }
        }"#;

        let err = validate_manifest(json).expect_err("Validation should fail");
        match err {
            ManifestError::ValidationError(msg) => {
                assert!(msg.contains("required") || msg.contains("artifacts"));
            }
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_invalid_manifest_pattern() {
        let json = r#"{
            "rule": {
                "name": "Invalid Name",
                "version": "1.0.0"
            },
            "artifacts": {
                "wasm": "https://example.com/rule.wasm",
                "sha256": "a3b6408225010668045610815132640108602685710662650426543168015505"
            }
        }"#;

        let err = validate_manifest(json).expect_err("Validation should fail");
        if let ManifestError::ValidationError(msg) = err {
            // Pattern mismatch for "name"
            assert!(msg.contains("name"));
        } else {
            panic!("Unexpected error type");
        }
    }

    #[test]
    fn test_is_valid_rule_name_basic() {
        assert!(is_valid_rule_name("no-todo"));
        assert!(is_valid_rule_name("sentence-length"));
        assert!(is_valid_rule_name("a"));
        assert!(is_valid_rule_name("rule123"));
        assert!(is_valid_rule_name("MyRule"));
        assert!(is_valid_rule_name("NO_TODO"));
    }

    #[test]
    fn test_is_valid_rule_name_multilingual() {
        assert!(is_valid_rule_name("敬語チェック"));
        assert!(is_valid_rule_name("冗長表現"));
        assert!(is_valid_rule_name("句子长度"));
        assert!(is_valid_rule_name("문장길이"));
        assert!(is_valid_rule_name("проверка"));
        assert!(is_valid_rule_name("Überprüfung"));
    }

    #[test]
    fn test_is_valid_rule_name_invalid_empty() {
        assert!(!is_valid_rule_name(""));
    }

    #[test]
    fn test_is_valid_rule_name_invalid_whitespace() {
        assert!(!is_valid_rule_name("my rule"));
        assert!(!is_valid_rule_name("my\trule"));
        assert!(!is_valid_rule_name("my\nrule"));
        assert!(!is_valid_rule_name(" myrule"));
        assert!(!is_valid_rule_name("myrule "));
    }

    #[test]
    fn test_is_valid_rule_name_invalid_control_chars() {
        assert!(!is_valid_rule_name("my\x00rule"));
        assert!(!is_valid_rule_name("my\x1Frule"));
        assert!(!is_valid_rule_name("my\x7Frule"));
        assert!(!is_valid_rule_name("my\u{9F}rule"));
    }

    #[test]
    fn test_is_valid_rule_name_max_length() {
        let max_name = "a".repeat(64);
        assert!(is_valid_rule_name(&max_name));

        let too_long = "a".repeat(65);
        assert!(!is_valid_rule_name(&too_long));
    }
}
