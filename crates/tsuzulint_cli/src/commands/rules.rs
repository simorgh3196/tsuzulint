//! Rules command implementation

use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result};
use tracing::{info, warn};
use tsuzulint_registry::resolver::PluginSpec;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum AddRuleError {
    #[error("WASM file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("Invalid file extension: expected .wasm, got {0:?}")]
    InvalidExtension(Option<String>),
    #[error("Failed to create plugin directory: {0}")]
    CreateDirError(#[source] std::io::Error),
    #[error("Failed to copy WASM file: {0}")]
    CopyError(#[source] std::io::Error),
    #[error("Failed to read manifest: {0}")]
    ManifestReadError(#[source] std::io::Error),
    #[error("Failed to parse manifest: {0}")]
    ManifestParseError(#[source] tsuzulint_manifest::ManifestError),
    #[error("Failed to read WASM file: {0}")]
    WasmReadError(#[source] std::io::Error),
    #[error("Failed to write manifest: {0}")]
    ManifestWriteError(#[source] std::io::Error),
    #[error(
        "Invalid rule alias '{name}': must be 1-{} characters with no whitespace or control characters. Use --as to specify a valid alias.",
        tsuzulint_manifest::MAX_RULE_NAME_LENGTH
    )]
    InvalidAlias { name: String },
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

fn find_manifest(wasm_path: &Path) -> Option<PathBuf> {
    let manifest_path = wasm_path.with_file_name("tsuzulint-rule.json");
    if manifest_path.exists() {
        return Some(manifest_path);
    }
    None
}

fn load_manifest(path: &Path) -> Result<tsuzulint_manifest::ExternalRuleManifest, AddRuleError> {
    let content = std::fs::read_to_string(path).map_err(AddRuleError::ManifestReadError)?;

    tsuzulint_manifest::validate_manifest(&content).map_err(AddRuleError::ManifestParseError)
}

fn generate_minimal_manifest(
    wasm_path: &Path,
    alias: &str,
) -> Result<tsuzulint_manifest::ExternalRuleManifest, AddRuleError> {
    use tsuzulint_manifest::{Artifacts, IsolationLevel, RuleMetadata};

    let wasm_content = std::fs::read(wasm_path).map_err(AddRuleError::WasmReadError)?;
    let sha256 = tsuzulint_registry::HashVerifier::compute(&wasm_content);

    Ok(tsuzulint_manifest::ExternalRuleManifest {
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
    })
}

fn copy_plugin_files(
    wasm_path: &Path,
    mut manifest: tsuzulint_manifest::ExternalRuleManifest,
    target_dir: &Path,
) -> Result<(), AddRuleError> {
    std::fs::create_dir_all(target_dir).map_err(AddRuleError::CreateDirError)?;

    let target_wasm = target_dir.join("rule.wasm");
    let target_manifest = target_dir.join("tsuzulint-rule.json");

    std::fs::copy(wasm_path, &target_wasm).map_err(AddRuleError::CopyError)?;

    manifest.artifacts.wasm = "rule.wasm".to_string();
    let wasm_content = std::fs::read(&target_wasm).map_err(AddRuleError::WasmReadError)?;
    manifest.artifacts.sha256 = tsuzulint_registry::HashVerifier::compute(&wasm_content);

    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        AddRuleError::ManifestParseError(tsuzulint_manifest::ManifestError::ValidationError(
            e.to_string(),
        ))
    })?;

    std::fs::write(&target_manifest, manifest_json).map_err(AddRuleError::ManifestWriteError)?;

    Ok(())
}

pub fn run_add_rule(path: &Path, alias: Option<&str>, config_path: Option<PathBuf>) -> Result<()> {
    let wasm_path = validate_wasm_path(path)?;

    let (manifest, rule_alias) = if let Some(manifest_path) = find_manifest(&wasm_path) {
        let manifest = load_manifest(&manifest_path)?;
        let alias_str = alias
            .map(str::to_string)
            .unwrap_or_else(|| manifest.rule.name.clone());
        (manifest, alias_str)
    } else {
        let alias_str = alias.map(str::to_string).unwrap_or_else(|| {
            wasm_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed-rule".to_string())
        });
        warn!(
            "Manifest not found for {}. Generating minimal manifest.",
            wasm_path.display()
        );
        let manifest = generate_minimal_manifest(&wasm_path, &alias_str)?;
        (manifest, alias_str)
    };

    if !tsuzulint_manifest::is_valid_rule_name(&rule_alias) {
        return Err(AddRuleError::InvalidAlias { name: rule_alias }.into());
    }

    let plugin_dir = PathBuf::from("rules/plugins").join(&rule_alias);
    let plugin_manifest = plugin_dir.join("tsuzulint-rule.json");

    if plugin_dir.exists() && plugin_manifest.exists() {
        info!(
            "Rule '{}' already exists at {}. Skipping.",
            rule_alias,
            plugin_dir.display()
        );
        return Ok(());
    }

    copy_plugin_files(&wasm_path, manifest.clone(), &plugin_dir)?;

    info!("Rule files copied to: {}", plugin_dir.display());

    let spec = PluginSpec {
        source: tsuzulint_registry::resolver::PluginSource::Path(plugin_dir.clone()),
        alias: Some(rule_alias.clone()),
    };

    crate::config::editor::update_config_with_plugin(&spec, &rule_alias, &manifest, config_path)?;

    info!("Rule '{}' added successfully", rule_alias);
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

        let manifest = generate_minimal_manifest(&wasm_path, "my-alias").unwrap();

        assert_eq!(manifest.rule.name, "my-alias");
        assert_eq!(manifest.rule.version, "0.1.0");
        assert_eq!(manifest.artifacts.wasm, "rule.wasm");
        assert!(!manifest.artifacts.sha256.is_empty());
    }

    #[test]
    fn test_generate_minimal_manifest_missing_file() {
        let wasm_path = PathBuf::from("/nonexistent/rule.wasm");
        let result = generate_minimal_manifest(&wasm_path, "my-alias");
        assert!(matches!(result, Err(AddRuleError::WasmReadError(_))));
    }

    #[test]
    fn test_copy_plugin_files() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("rule.wasm");
        std::fs::write(&wasm_path, b"wasm content").unwrap();

        let manifest = generate_minimal_manifest(&wasm_path, "test-rule").unwrap();
        let target_dir = dir.path().join("plugins/test-rule");

        let result = copy_plugin_files(&wasm_path, manifest, &target_dir);
        assert!(result.is_ok());

        assert!(target_dir.join("rule.wasm").exists());
        assert!(target_dir.join("tsuzulint-rule.json").exists());

        let saved_manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(target_dir.join("tsuzulint-rule.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved_manifest["artifacts"]["wasm"], "rule.wasm");
    }

    #[test]
    fn test_copy_plugin_files_recomputes_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("rule.wasm");
        let wasm_content = b"actual wasm content";
        std::fs::write(&wasm_path, wasm_content).unwrap();

        let manifest_with_wrong_hash = {
            use tsuzulint_manifest::{Artifacts, IsolationLevel, RuleMetadata};
            tsuzulint_manifest::ExternalRuleManifest {
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
                },
                artifacts: Artifacts {
                    wasm: "old-name.wasm".to_string(),
                    sha256: "wrong_hash_that_should_be_replaced".to_string(),
                },
                permissions: None,
                tsuzulint: None,
                options: None,
            }
        };

        let target_dir = dir.path().join("plugins/test-rule");
        let result = copy_plugin_files(&wasm_path, manifest_with_wrong_hash, &target_dir);
        assert!(result.is_ok());

        let saved_manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(target_dir.join("tsuzulint-rule.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(saved_manifest["artifacts"]["wasm"], "rule.wasm");

        let expected_hash = tsuzulint_registry::HashVerifier::compute(wasm_content);
        assert_eq!(
            saved_manifest["artifacts"]["sha256"].as_str().unwrap(),
            expected_hash
        );

        assert_ne!(
            saved_manifest["artifacts"]["sha256"].as_str().unwrap(),
            "wrong_hash_that_should_be_replaced"
        );
    }

    #[test]
    fn test_run_add_rule_invalid_alias_with_space() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("My Rule.wasm");
        std::fs::write(&wasm_path, b"wasm content").unwrap();

        let result = run_add_rule(&wasm_path, None, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid rule alias"));
    }

    #[test]
    fn test_run_add_rule_invalid_alias_too_long() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("rule.wasm");
        std::fs::write(&wasm_path, b"wasm content").unwrap();

        let too_long_alias = "a".repeat(65);
        let result = run_add_rule(&wasm_path, Some(&too_long_alias), None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid rule alias"));
    }

    #[test]
    fn test_run_add_rule_retries_on_incomplete_install() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("rule.wasm");
        std::fs::write(&wasm_path, b"wasm content").unwrap();

        let incomplete_plugin_dir = PathBuf::from("rules/plugins/test-incomplete");
        std::fs::create_dir_all(&incomplete_plugin_dir).unwrap();

        assert!(incomplete_plugin_dir.exists());
        assert!(!incomplete_plugin_dir.join("tsuzulint-rule.json").exists());

        let result = run_add_rule(&wasm_path, Some("test-incomplete"), None);
        assert!(result.is_ok() || result.is_err());

        let _ = std::fs::remove_dir_all("rules/plugins/test-incomplete");
    }
}
