use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use thiserror::Error;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IsolationLevel {
    #[default]
    Global,
    Block,
}

fn default_isolation_level() -> IsolationLevel {
    IsolationLevel::Global
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifacts {
    pub wasm: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub filesystem: Vec<FilesystemPermission>,
    #[serde(default)]
    pub network: Vec<NetworkPermission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPermission {
    pub path: String,
    pub access: String, // "read" or "write"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPermission {
    pub host: String,
    pub access: String, // "http" or "https"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsuzuLintCompatibility {
    pub min_version: Option<String>,
}

// Embed the schema
const RULE_SCHEMA_JSON: &str = include_str!("../../../schemas/v1/rule.json");

static SCHEMA: OnceLock<Validator> = OnceLock::new();

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
}
