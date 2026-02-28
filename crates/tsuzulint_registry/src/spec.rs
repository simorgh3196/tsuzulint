//! Plugin specification parsing.

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;

use crate::fetcher::PluginSource;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid plugin specification format")]
    InvalidFormat,
    #[error("Missing alias ('as') for {src} source")]
    MissingAlias { src: String },
    #[error("Invalid object format: {0}")]
    InvalidObject(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    pub source: PluginSource,
    pub alias: Option<String>,
}

impl PluginSpec {
    pub fn parse(value: &Value) -> Result<Self, ParseError> {
        match value {
            Value::String(s) => Self::parse_string(s),
            Value::Object(_) => Self::parse_object(value),
            _ => Err(ParseError::InvalidFormat),
        }
    }

    fn parse_string(s: &str) -> Result<Self, ParseError> {
        let parts: Vec<&str> = s.split('@').collect();
        let (name_part, version) = match parts.len() {
            1 => (parts[0], None),
            2 => {
                let v = parts[1].trim();
                if v.is_empty() {
                    return Err(ParseError::InvalidFormat);
                }
                (parts[0], Some(v.to_string()))
            }
            _ => return Err(ParseError::InvalidFormat),
        };

        let name_parts: Vec<&str> = name_part.split('/').collect();
        if name_parts.len() != 2 {
            return Err(ParseError::InvalidFormat);
        }

        let owner = name_parts[0].trim();
        let repo = name_parts[1].trim();

        if owner.is_empty() || repo.is_empty() {
            return Err(ParseError::InvalidFormat);
        }

        Ok(Self {
            source: PluginSource::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
                version,
                server_url: None,
            },
            alias: None,
        })
    }

    fn parse_object(value: &Value) -> Result<Self, ParseError> {
        #[derive(Deserialize)]
        struct SpecObj {
            github: Option<String>,
            url: Option<String>,
            path: Option<String>,
            server_url: Option<String>,
            #[serde(rename = "as")]
            alias: Option<String>,
        }

        let obj: SpecObj = serde_json::from_value(value.clone())
            .map_err(|e| ParseError::InvalidObject(e.to_string()))?;

        let sources_count = [obj.github.is_some(), obj.url.is_some(), obj.path.is_some()]
            .into_iter()
            .filter(|&x| x)
            .count();

        if sources_count != 1 {
            return Err(ParseError::InvalidObject(
                "Exactly one of 'github', 'url', or 'path' must be specified".to_string(),
            ));
        }

        if let Some(github) = obj.github {
            let mut spec = Self::parse_string(&github)?;
            if let PluginSource::GitHub { server_url, .. } = &mut spec.source {
                *server_url = obj.server_url;
            }
            return Ok(Self {
                source: spec.source,
                alias: obj.alias,
            });
        }

        if let Some(url) = obj.url {
            if obj.alias.is_none() {
                return Err(ParseError::MissingAlias {
                    src: "url".to_string(),
                });
            }
            return Ok(Self {
                source: PluginSource::Url(url),
                alias: obj.alias,
            });
        }

        if let Some(path_str) = obj.path {
            return Ok(Self {
                source: PluginSource::Path(PathBuf::from(path_str)),
                alias: obj.alias,
            });
        }

        Err(ParseError::InvalidFormat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_string_github_latest() {
        let value = json!("owner/repo");
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: None,
                server_url: None,
            }
        );
        assert_eq!(spec.alias, None);
    }

    #[test]
    fn test_parse_string_github_version() {
        let value = json!("owner/repo@v1.2.3");
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: Some("v1.2.3".to_string()),
                server_url: None,
            }
        );
    }

    #[test]
    fn test_parse_object_github() {
        let value = json!({
            "github": "owner/repo",
            "as": "my-rule"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: None,
                server_url: None,
            }
        );
        assert_eq!(spec.alias, Some("my-rule".to_string()));
    }

    #[test]
    fn test_parse_object_github_with_server_url() {
        let value = json!({
            "github": "owner/repo",
            "server_url": "https://git.internal.example.com",
            "as": "my-rule"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: None,
                server_url: Some("https://git.internal.example.com".to_string()),
            }
        );
        assert_eq!(spec.alias, Some("my-rule".to_string()));
    }

    #[test]
    fn test_parse_object_url() {
        let value = json!({
            "url": "https://example.com/manifest.json",
            "as": "rule-alias"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::Url("https://example.com/manifest.json".to_string())
        );
        assert_eq!(spec.alias, Some("rule-alias".to_string()));
    }

    #[test]
    fn test_parse_object_path() {
        let value = json!({
            "path": "./local/rule",
            "as": "local-rule"
        });
        let spec = PluginSpec::parse(&value).unwrap();
        assert_eq!(
            spec.source,
            PluginSource::Path(PathBuf::from("./local/rule"))
        );
        assert_eq!(spec.alias, Some("local-rule".to_string()));
    }

    #[test]
    fn test_parse_error_missing_alias_url() {
        let value = json!({ "url": "https://example.com" });
        let result = PluginSpec::parse(&value);
        assert!(matches!(result, Err(ParseError::MissingAlias { .. })));
    }

    #[test]
    fn test_parse_object_path_optional_alias() {
        let value = json!({
            "path": "./local/rule"
        });
        let spec = PluginSpec::parse(&value).expect("Parsing should succeed");
        assert_eq!(spec.alias, None);
    }

    #[test]
    fn test_parse_error_invalid_string() {
        assert!(PluginSpec::parse(&json!("invalid")).is_err());
        assert!(PluginSpec::parse(&json!("owner/repo/extra")).is_err());
        assert!(PluginSpec::parse(&json!("owner/repo@v1@v2")).is_err());
        assert!(PluginSpec::parse(&json!("/repo")).is_err());
        assert!(PluginSpec::parse(&json!("owner/")).is_err());
        assert!(PluginSpec::parse(&json!("/")).is_err());
    }

    #[test]
    fn test_parse_string_error_empty_version() {
        assert!(matches!(
            PluginSpec::parse(&json!("owner/repo@")),
            Err(ParseError::InvalidFormat)
        ));
        assert!(matches!(
            PluginSpec::parse(&json!("owner/repo@   ")),
            Err(ParseError::InvalidFormat)
        ));
    }

    #[test]
    fn test_parse_object_error_multiple_sources() {
        let value = json!({
            "github": "owner/repo",
            "url": "https://example.com/manifest.json",
            "as": "alias"
        });
        match PluginSpec::parse(&value) {
            Err(ParseError::InvalidObject(msg)) => {
                assert!(msg.contains("Exactly one"));
            }
            _ => panic!("Should fail with InvalidObject"),
        }
    }

    #[test]
    fn test_parse_object_error_no_source() {
        let value = json!({
            "as": "alias"
        });
        match PluginSpec::parse(&value) {
            Err(ParseError::InvalidObject(msg)) => {
                assert!(msg.contains("Exactly one"));
            }
            _ => panic!("Should fail with InvalidObject"),
        }
    }
}
