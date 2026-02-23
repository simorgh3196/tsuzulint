//! Rules command implementation

use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result};
use tracing::{info, warn};
use tsuzulint_registry::resolver::PluginSpec;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AddRuleError {
    #[error("WASM file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("Invalid file extension: expected .wasm, got {0:?}")]
    InvalidExtension(Option<String>),
    #[error("Rule '{0}' already exists at {1}. Use a different alias with --as.")]
    AlreadyExists(String, PathBuf),
    #[error("Failed to create plugin directory: {0}")]
    CreateDirError(#[source] std::io::Error),
    #[error("Failed to copy WASM file: {0}")]
    CopyError(#[source] std::io::Error),
    #[error("Failed to read manifest: {0}")]
    ManifestReadError(#[source] std::io::Error),
    #[error("Failed to parse manifest: {0}")]
    ManifestParseError(#[source] tsuzulint_manifest::ManifestError),
}

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

#[allow(dead_code)]
fn validate_wasm_path(path: &Path) -> Result<PathBuf, AddRuleError> {
    if !path.exists() {
        return Err(AddRuleError::FileNotFound(path.to_path_buf()));
    }

    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    if ext.as_deref() != Some("wasm") {
        return Err(AddRuleError::InvalidExtension(ext));
    }

    path.canonicalize()
        .map_err(|_| AddRuleError::FileNotFound(path.to_path_buf()))
}

#[allow(dead_code)]
fn find_manifest(wasm_path: &Path) -> Option<PathBuf> {
    let wasm_dir = wasm_path.parent()?;

    let candidates = [
        wasm_path.with_file_name("tsuzulint-rule.json"),
        wasm_dir.join("tsuzulint-rule.json"),
    ];

    candidates.into_iter().find(|candidate| candidate.exists())
}

#[allow(dead_code)]
fn load_manifest(path: &Path) -> Result<tsuzulint_manifest::ExternalRuleManifest, AddRuleError> {
    let content = std::fs::read_to_string(path).map_err(AddRuleError::ManifestReadError)?;

    tsuzulint_manifest::validate_manifest(&content).map_err(AddRuleError::ManifestParseError)
}

#[allow(dead_code)]
fn generate_minimal_manifest(
    wasm_path: &Path,
    alias: &str,
) -> tsuzulint_manifest::ExternalRuleManifest {
    use tsuzulint_manifest::{Artifacts, IsolationLevel, RuleMetadata};

    let wasm_content = std::fs::read(wasm_path).unwrap_or_default();
    let sha256 = tsuzulint_registry::HashVerifier::compute(&wasm_content);

    tsuzulint_manifest::ExternalRuleManifest {
        rule: RuleMetadata {
            name: alias.to_string(),
            version: "0.1.0".to_string(),
            description: None,
            repository: None,
            license: None,
            authors: vec![],
            keywords: vec![],
            fixable: false,
            node_types: vec![],
            isolation_level: IsolationLevel::Global,
        },
        artifacts: Artifacts {
            wasm: "rule.wasm".to_string(),
            sha256,
        },
        permissions: None,
        tsuzulint: None,
        options: None,
    }
}

pub fn run_add_rule(
    path: &Path,
    _alias: Option<&str>,
    _config_path: Option<PathBuf>,
) -> Result<()> {
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

#[cfg(test)]
mod add_rule_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_wasm_path_valid() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().with_extension("wasm");
        file.persist(&path).unwrap();

        let result = validate_wasm_path(&path);
        assert!(result.is_ok());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_validate_wasm_path_not_found() {
        let result = validate_wasm_path(Path::new("nonexistent.wasm"));
        assert!(matches!(result, Err(AddRuleError::FileNotFound(_))));
    }

    #[test]
    fn test_validate_wasm_path_invalid_extension() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test").unwrap();

        let result = validate_wasm_path(file.path());
        assert!(matches!(result, Err(AddRuleError::InvalidExtension(_))));
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_find_manifest_sibling() {
        let dir = TempDir::new().unwrap();
        let wasm_path = dir.path().join("rule.wasm");
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        std::fs::write(&wasm_path, b"wasm").unwrap();
        std::fs::write(&manifest_path, b"{}").unwrap();

        let found = find_manifest(&wasm_path);
        assert_eq!(found, Some(manifest_path));
    }

    #[test]
    fn test_find_manifest_none() {
        let dir = TempDir::new().unwrap();
        let wasm_path = dir.path().join("rule.wasm");
        std::fs::write(&wasm_path, b"wasm").unwrap();

        let found = find_manifest(&wasm_path);
        assert!(found.is_none());
    }

    #[test]
    fn test_load_manifest_valid() {
        let dir = TempDir::new().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        let valid_manifest = r#"{
            "rule": {
                "name": "test-rule",
                "version": "1.0.0"
            },
            "artifacts": {
                "wasm": "rule.wasm",
                "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            }
        }"#;
        std::fs::write(&manifest_path, valid_manifest).unwrap();

        let result = load_manifest(&manifest_path);
        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.rule.name, "test-rule");
        assert_eq!(manifest.rule.version, "1.0.0");
    }

    #[test]
    fn test_load_manifest_invalid_json() {
        let dir = TempDir::new().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");

        std::fs::write(&manifest_path, b"not valid json").unwrap();

        let result = load_manifest(&manifest_path);
        assert!(matches!(result, Err(AddRuleError::ManifestParseError(_))));
    }

    #[test]
    fn test_generate_minimal_manifest() {
        let dir = TempDir::new().unwrap();
        let wasm_path = dir.path().join("rule.wasm");

        let wasm_content = b"\x00asm\x01\x00\x00\x00";
        std::fs::write(&wasm_path, wasm_content).unwrap();

        let manifest = generate_minimal_manifest(&wasm_path, "my-alias");

        assert_eq!(manifest.rule.name, "my-alias");
        assert_eq!(manifest.rule.version, "0.1.0");
        assert_eq!(manifest.artifacts.wasm, "rule.wasm");
        assert!(!manifest.artifacts.sha256.is_empty());
    }
}
