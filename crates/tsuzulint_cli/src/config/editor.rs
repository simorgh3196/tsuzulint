//! Configuration file editing logic

use std::path::PathBuf;

use jsonc_parser::ast::ObjectPropName;
use jsonc_parser::{CollectOptions, ParseOptions};
use miette::{IntoDiagnostic, Result};
use tracing::info;
use tsuzulint_core::LinterConfig;
use tsuzulint_registry::manifest::ExternalRuleManifest;
use tsuzulint_registry::resolver::PluginSpec;

use crate::commands::init::run_init;

struct ConfigEdit {
    pos: usize,
    text: String,
    order: usize,
}

pub fn update_config_with_plugin(
    spec: &PluginSpec,
    alias: &str,
    manifest: &ExternalRuleManifest,
    config_path: Option<PathBuf>,
) -> Result<()> {
    let path_to_use = if let Some(path) = config_path {
        path
    } else if let Some(path) = LinterConfig::discover(".") {
        path
    } else {
        run_init(false)?;
        PathBuf::from(LinterConfig::CONFIG_FILES[0])
    };

    if std::fs::symlink_metadata(&path_to_use).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(miette::miette!(
            "Refusing to modify configuration file because it is a symbolic link: {}",
            path_to_use.display()
        ));
    }

    let content = std::fs::read_to_string(&path_to_use).into_diagnostic()?;

    let parse_options = ParseOptions::default();
    let collect_options = CollectOptions::default();
    let ast = jsonc_parser::parse_to_ast(&content, &collect_options, &parse_options)
        .map_err(|e| miette::miette!("Failed to parse config: {}", e))?;

    let config_value: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&content, &parse_options)
            .map_err(|e| miette::miette!("Failed to parse config: {}", e))?
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let mut edits = Vec::new();

    let rule_def_str = generate_rule_def(spec, alias, manifest)?;
    let options_str = generate_options_def(alias, manifest)?;

    let rule_def_json: serde_json::Value = serde_json::from_str(&rule_def_str).into_diagnostic()?;
    let needs_add_rule = if let Some(rules) = config_value.get("rules").and_then(|v| v.as_array()) {
        !rules.contains(&rule_def_json)
    } else {
        true
    };

    let root_obj = ast
        .value
        .as_ref()
        .and_then(|v| v.as_object())
        .ok_or_else(|| miette::miette!("Invalid config: root must be an object"))?;

    let mut needs_add_rule_prop = false;

    if needs_add_rule {
        let rules_prop = root_obj.properties.iter().find(|p| match &p.name {
            ObjectPropName::String(s) => s.value == "rules",
            ObjectPropName::Word(w) => w.value == "rules",
        });

        if let Some(prop) = rules_prop {
            if let Some(array) = prop.value.as_array() {
                let end_pos = array.range.end - 1;
                let is_empty = array.elements.is_empty();
                let prefix = if is_empty { "\n    " } else { ",\n    " };
                edits.push(ConfigEdit {
                    pos: end_pos,
                    text: format!("{}\n  ", format!("{}{}", prefix, rule_def_str).trim_end()),
                    order: 0,
                });
            } else {
                return Err(miette::miette!("Invalid config: 'rules' must be an array"));
            }
        } else {
            needs_add_rule_prop = true;
        }
    }

    let needs_add_option = config_value
        .get("options")
        .and_then(|v| v.as_object())
        .map(|o| !o.contains_key(alias))
        .unwrap_or(true);

    let mut needs_add_option_prop = false;

    if needs_add_option {
        let options_prop = root_obj.properties.iter().find(|p| match &p.name {
            ObjectPropName::String(s) => s.value == "options",
            ObjectPropName::Word(w) => w.value == "options",
        });

        if let Some(prop) = options_prop {
            if let Some(obj) = prop.value.as_object() {
                let end_pos = obj.range.end - 1;
                let is_empty = obj.properties.is_empty();
                let prefix = if is_empty { "\n    " } else { ",\n    " };
                edits.push(ConfigEdit {
                    pos: end_pos,
                    text: format!("{}\n  ", format!("{}{}", prefix, options_str).trim_end()),
                    order: 0,
                });
            } else {
                return Err(miette::miette!(
                    "Invalid config: 'options' must be an object"
                ));
            }
        } else {
            needs_add_option_prop = true;
        }
    }

    let mut root_props_to_add: Vec<String> = Vec::new();
    if needs_add_rule_prop {
        root_props_to_add.push(format!("\"rules\": [\n    {}\n  ]", rule_def_str));
    }
    if needs_add_option_prop {
        root_props_to_add.push(format!("\"options\": {{\n    {}\n  }}", options_str));
    }

    if !root_props_to_add.is_empty() {
        let root_end = root_obj.range.end - 1;
        let has_existing = !root_obj.properties.is_empty();
        let count = root_props_to_add.len();

        for (i, text) in root_props_to_add.into_iter().enumerate() {
            let need_comma = has_existing || i > 0;
            let prefix = if need_comma { ",\n  " } else { "\n  " };

            let is_last = i + 1 == count;
            let suffix = if is_last { "\n" } else { "" };

            edits.push(ConfigEdit {
                pos: root_end,
                text: format!("{}{}{}", prefix, text, suffix),
                order: i,
            });
        }
    }

    edits.sort_by(|a, b| b.pos.cmp(&a.pos).then(b.order.cmp(&a.order)));

    let mut new_content = content;
    for edit in edits {
        new_content.insert_str(edit.pos, &edit.text);
    }

    std::fs::write(&path_to_use, new_content).into_diagnostic()?;
    info!("Updated {}", path_to_use.display());
    Ok(())
}

fn generate_rule_def(
    spec: &PluginSpec,
    alias: &str,
    manifest: &ExternalRuleManifest,
) -> Result<String> {
    use tsuzulint_registry::resolver::PluginSource;

    match &spec.source {
        PluginSource::GitHub { owner, repo, .. } => {
            let version = &manifest.rule.version;
            let source_str = format!("{}/{}@{}", owner, repo, version);
            let source_json = serde_json::to_string(&source_str).into_diagnostic()?;
            if let Some(a) = &spec.alias {
                let alias_json = serde_json::to_string(a).into_diagnostic()?;
                Ok(format!(
                    "{{\n      \"github\": {},\n      \"as\": {}\n    }}",
                    source_json, alias_json
                ))
            } else {
                Ok(source_json)
            }
        }
        PluginSource::Url(url) => {
            let url_json = serde_json::to_string(url).into_diagnostic()?;
            let alias_json = serde_json::to_string(alias).into_diagnostic()?;
            Ok(format!(
                "{{\n      \"url\": {},\n      \"as\": {}\n    }}",
                url_json, alias_json
            ))
        }
        PluginSource::Path(path) => {
            let path_json = serde_json::to_string(path).into_diagnostic()?;
            let alias_json = serde_json::to_string(alias).into_diagnostic()?;
            Ok(format!(
                "{{\n      \"path\": {},\n      \"as\": {}\n    }}",
                path_json, alias_json
            ))
        }
    }
}

fn generate_options_def(alias: &str, manifest: &ExternalRuleManifest) -> Result<String> {
    let default_options = if let Some(opts) = &manifest.options {
        opts.clone()
    } else {
        serde_json::Value::Bool(true)
    };

    let alias_json = serde_json::to_string(alias).into_diagnostic()?;
    let options_json = serde_json::to_string(&default_options).into_diagnostic()?;
    Ok(format!(r#"{}: {}"#, alias_json, options_json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use extism_manifest::{Wasm, WasmMetadata};
    use std::io::Write;
    use tsuzulint_registry::manifest::{ExternalRuleManifest, IsolationLevel, RuleMetadata};
    use tsuzulint_registry::resolver::{PluginSource, PluginSpec};

    fn create_dummy_manifest() -> ExternalRuleManifest {
        ExternalRuleManifest {
            rule: RuleMetadata {
                name: "test-rule".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                repository: None,
                license: None,
                authors: vec![],
                keywords: vec![],
                fixable: false,
                node_types: vec![],
                isolation_level: IsolationLevel::Global,
                languages: vec![],
                capabilities: vec![],
            },
            wasm: vec![Wasm::File {
                path: std::path::PathBuf::from("test.wasm"),
                meta: WasmMetadata {
                    name: None,
                    hash: Some(
                        "1111111111111111111111111111111111111111111111111111111111111111"
                            .to_string(),
                    ),
                },
            }],
            allowed_hosts: None,
            allowed_paths: None,
            config: std::collections::BTreeMap::new(),
            memory: None,
            timeout_ms: None,
            tsuzulint: None,
            options: Some(serde_json::json!({ "foo": "bar" })),
        }
    }

    fn create_dummy_spec() -> PluginSpec {
        PluginSpec {
            source: PluginSource::GitHub {
                owner: "owner".to_string(),
                repo: "repo".to_string(),
                version: None,
                server_url: None,
            },
            alias: None,
        }
    }

    #[test]
    fn test_update_config_empty_object_adds_rules_and_options() {
        let manifest = create_dummy_manifest();
        let spec = create_dummy_spec();
        let alias = "test-alias";

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        write!(temp_file, "{{}}").unwrap();
        let path = temp_file.path().to_path_buf();

        update_config_with_plugin(&spec, alias, &manifest, Some(path.clone())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        let rules = json["rules"].as_array().expect("rules should be an array");
        assert!(rules.iter().any(|r| {
            if let Some(s) = r.as_str() {
                s == "owner/repo@1.0.0"
            } else {
                false
            }
        }));

        let options = json["options"]
            .as_object()
            .expect("options should be an object");
        assert_eq!(options["test-alias"]["foo"], "bar");
    }

    #[test]
    fn test_update_config_existing_rules_appends_rule_and_adds_options() {
        let manifest = create_dummy_manifest();
        let spec = create_dummy_spec();
        let alias = "test-alias";

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        write!(
            temp_file,
            r#"{{
  "rules": [
    "existing/rule"
  ]
}}"#
        )
        .unwrap();
        let path = temp_file.path().to_path_buf();

        update_config_with_plugin(&spec, alias, &manifest, Some(path.clone())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        let rules = json["rules"].as_array().expect("rules should be an array");
        assert!(rules.contains(&serde_json::Value::String("existing/rule".to_string())));
        assert!(rules.iter().any(|r| {
            if let Some(s) = r.as_str() {
                s == "owner/repo@1.0.0"
            } else {
                false
            }
        }));

        assert!(json["options"].as_object().is_some());
    }

    #[test]
    fn test_update_config_existing_options_adds_rules_and_preserves_options() {
        let manifest = create_dummy_manifest();
        let spec = create_dummy_spec();
        let alias = "test-alias";

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        write!(
            temp_file,
            r#"{{
  "options": {{
    "existing": true
  }}
}}"#
        )
        .unwrap();
        let path = temp_file.path().to_path_buf();

        update_config_with_plugin(&spec, alias, &manifest, Some(path.clone())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        assert_eq!(json["options"]["existing"], true);
        assert!(json["rules"].as_array().is_some());
        let rules = json["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| {
            if let Some(s) = r.as_str() {
                s == "owner/repo@1.0.0"
            } else {
                false
            }
        }));
        assert_eq!(json["options"]["test-alias"]["foo"], "bar");
    }

    #[test]
    fn test_update_config_existing_both_appends_rule_and_option() {
        let manifest = create_dummy_manifest();
        let spec = create_dummy_spec();
        let alias = "test-alias";

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        write!(
            temp_file,
            r#"{{
  "rules": [],
  "options": {{}}
}}"#
        )
        .unwrap();
        let path = temp_file.path().to_path_buf();

        update_config_with_plugin(&spec, alias, &manifest, Some(path.clone())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        let rules = json["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| {
            if let Some(s) = r.as_str() {
                s == "owner/repo@1.0.0"
            } else {
                false
            }
        }));
        assert_eq!(json["options"]["test-alias"]["foo"], "bar");
    }

    #[test]
    #[cfg_attr(
        windows,
        ignore = "Cannot create symlink without privileges on Windows CI"
    )]
    fn test_update_config_refuses_symlink() {
        use tempfile::NamedTempFile;

        let target_file = NamedTempFile::new().unwrap();
        let target_path = target_file.path();

        let symlink_path = target_path.parent().unwrap().join("symlink_config.json");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_path, &symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target_path, &symlink_path).unwrap();

        let spec = create_dummy_spec();
        let manifest = create_dummy_manifest();

        let result =
            update_config_with_plugin(&spec, "test-rule", &manifest, Some(symlink_path.clone()));

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Refusing to modify configuration file because it is a symbolic link")
        );
        assert!(msg.contains(&symlink_path.to_string_lossy().to_string()));

        let target_content = std::fs::read(target_path).unwrap();
        assert!(
            target_content.is_empty(),
            "Symlink target must not be modified"
        );

        let _ = std::fs::remove_file(&symlink_path);
    }
}
