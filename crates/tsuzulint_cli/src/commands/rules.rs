//! Rules command implementation

use std::path::Path;

use miette::{IntoDiagnostic, Result};
use tracing::{info, warn};
use tsuzulint_registry::resolver::PluginSpec;

pub fn run_create_rule(name: &str) -> Result<()> {
    let rule_dir = std::path::PathBuf::from(name);

    if rule_dir.exists() {
        return Err(miette::miette!("Directory '{}' already exists", name));
    }

    std::fs::create_dir_all(&rule_dir).into_diagnostic()?;

    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1.2"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
rmp-serde = "1.3"
"#,
        name.replace('-', "_")
    );

    std::fs::write(rule_dir.join("Cargo.toml"), cargo_toml).into_diagnostic()?;

    let lib_rs = format!(
        r#"//! {} rule for TsuzuLint

use extism_pdk::*;
use serde::{{Deserialize, Serialize}};

#[derive(Debug, Serialize)]
struct RuleManifest {{
    name: String,
    version: String,
    description: Option<String>,
    fixable: bool,
    node_types: Vec<String>,
}}

#[derive(Debug, Deserialize)]
struct LintRequest {{
    node: serde_json::Value,
    config: serde_json::Value,
    source: String,
    file_path: Option<String>,
}}

#[derive(Debug, Serialize)]
struct LintResponse {{
    diagnostics: Vec<Diagnostic>,
}}

#[derive(Debug, Serialize)]
struct Diagnostic {{
    rule_id: String,
    message: String,
    span: Span,
    severity: String,
}}

#[derive(Debug, Serialize)]
struct Span {{
    start: u32,
    end: u32,
}}

#[plugin_fn]
pub fn get_manifest() -> FnResult<String> {{
    let manifest = RuleManifest {{
        name: "{}".to_string(),
        version: "0.1.0".to_string(),
        description: Some("TODO: Add description".to_string()),
        fixable: false,
        node_types: vec!["Str".to_string()],
    }};

    Ok(serde_json::to_string(&manifest)?)
}}

#[plugin_fn]
pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>> {{
    let request: LintRequest = rmp_serde::from_slice(&input)?;
    let mut diagnostics = Vec::new();

    // TODO: Implement your rule logic here
    // Example: Check for specific patterns in text nodes
    //
    // if let Some(value) = request.node.get("value") {{
    //     if value.as_str().unwrap_or("").contains("TODO") {{
    //         diagnostics.push(Diagnostic {{
    //             rule_id: "{}".to_string(),
    //             message: "Found TODO".to_string(),
    //             span: Span {{ start: 0, end: 4 }},
    //             severity: "error".to_string(),
    //         }});
    //     }}
    // }}

    let response = LintResponse {{ diagnostics }};
    Ok(rmp_serde::to_vec_named(&response)?)
}}
"#,
        name, name, name
    );

    std::fs::create_dir_all(rule_dir.join("src")).into_diagnostic()?;
    std::fs::write(rule_dir.join("src/lib.rs"), lib_rs).into_diagnostic()?;

    info!("Created rule project: {}", name);
    info!(
        "To build: cd {} && cargo build --target wasm32-wasip1 --release",
        name
    );

    Ok(())
}

pub fn run_add_rule(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(miette::miette!("File not found: {}", path.display()));
    }

    info!("Rule added: {}", path.display());
    info!("Add the rule to your .tsuzulint.jsonc to enable it");

    Ok(())
}

/// Helper to build PluginSpec from RuleDefinition detail
pub fn build_spec_from_detail(
    key: &str,
    value: &str,
    alias: Option<&str>,
) -> (Option<PluginSpec>, Option<String>) {
    let mut map = serde_json::Map::new();
    map.insert(
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    );
    if let Some(a) = alias {
        map.insert("as".to_string(), serde_json::Value::String(a.to_string()));
    }
    let val = serde_json::Value::Object(map);
    match PluginSpec::parse(&val) {
        Ok(spec) => (Some(spec), alias.map(String::from)),
        Err(e) => {
            warn!(
                "Failed to parse rule detail (key={}, value={}): {}",
                key, value, e
            );
            (None, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsuzulint_registry::resolver::PluginSource;

    #[test]
    fn test_build_spec_from_detail() {
        let (spec, alias) = build_spec_from_detail("github", "owner/repo", Some("alias"));
        assert!(spec.is_some());
        assert!(matches!(
            spec.as_ref().unwrap().source,
            PluginSource::GitHub { .. }
        ));
        assert_eq!(alias, Some("alias".to_string()));

        let (spec, alias) = build_spec_from_detail("invalid", "value", None);
        assert!(spec.is_none());
        assert!(alias.is_none());
    }

    #[test]
    fn test_build_spec_from_detail_url() {
        let (spec, alias) =
            build_spec_from_detail("url", "https://example.com/rule.wasm", Some("my-rule"));
        assert!(spec.is_some());
        assert!(matches!(
            spec.as_ref().unwrap().source,
            PluginSource::Url(_)
        ));
        assert_eq!(alias, Some("my-rule".to_string()));
    }

    #[test]
    fn test_build_spec_from_detail_github_no_alias() {
        let (spec, alias) = build_spec_from_detail("github", "owner/repo", None);
        assert!(spec.is_some());
        assert!(matches!(
            spec.as_ref().unwrap().source,
            PluginSource::GitHub { .. }
        ));
        assert!(alias.is_none());
    }
}
