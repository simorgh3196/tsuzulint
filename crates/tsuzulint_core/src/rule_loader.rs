//! Rule loading and plugin host initialization.

use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{debug, warn};
use tsuzulint_plugin::PluginHost;
use tsuzulint_plugin::PluginOptions;

use crate::config::{LinterConfig, RuleDefinition};
use crate::manifest_resolver::resolve_manifest_path;
use crate::resolver::PluginResolver;
use crate::rule_manifest;

pub fn create_plugin_host(
    config: &LinterConfig,
    dynamic_rules: &Mutex<Vec<std::path::PathBuf>>,
) -> Result<PluginHost, crate::error::LinterError> {
    let mut host = PluginHost::new();

    load_configured_rules(config, &mut host);

    {
        let dynamic = dynamic_rules.lock().map_err(|_| {
            crate::error::LinterError::Internal("Dynamic rules mutex poisoned".to_string())
        })?;
        for path in dynamic.iter() {
            debug!("Loading dynamic plugin from {}", path.display());
            if let Err(e) = host.load_rule(path, PluginOptions::default()) {
                warn!("Failed to load dynamic plugin '{}': {}", path.display(), e);
            }
        }
    }

    Ok(host)
}

pub fn load_configured_rules(config: &LinterConfig, host: &mut PluginHost) {
    let load_plugin = |name: &str, host: &mut PluginHost| match PluginResolver::resolve(
        name,
        config.base_dir.as_deref(),
    ) {
        Some(path) => {
            debug!("Loading plugin '{}' from {}", name, path.display());
            if let Err(e) = host.load_rule(&path, PluginOptions::default()) {
                warn!("Failed to load plugin '{}': {}", name, e);
            }
        }
        None => {
            debug!(
                "Plugin '{}' not found. Checked .tsuzulint/plugins/ and global directories.",
                name
            );
        }
    };

    for rule_def in &config.rules {
        match rule_def {
            RuleDefinition::Simple(name) => {
                load_plugin(name, host);
            }
            RuleDefinition::Detail(detail) => {
                if let Some(path) = &detail.path {
                    if let Some(manifest_path) =
                        resolve_manifest_path(config.base_dir.as_deref(), path)
                    {
                        match rule_manifest::load_rule_manifest(&manifest_path) {
                            Ok(result) => {
                                let rule_name = detail
                                    .r#as
                                    .clone()
                                    .unwrap_or_else(|| result.manifest.rule.name.clone());
                                debug!(
                                    "Loading rule '{}' from manifest: {}",
                                    rule_name,
                                    manifest_path.display()
                                );
                                let options = PluginOptions {
                                    allowed_hosts: result.manifest.allowed_hosts.clone(),
                                    allowed_paths: result.manifest.allowed_paths.clone(),
                                    config: result.manifest.config.clone(),
                                    memory_max_pages: result
                                        .manifest
                                        .memory
                                        .as_ref()
                                        .and_then(|m| m.max_pages),
                                    memory_max_http_response_bytes: result
                                        .manifest
                                        .memory
                                        .as_ref()
                                        .and_then(|m| m.max_http_response_bytes),
                                    timeout_ms: result.manifest.timeout_ms,
                                };
                                match host.load_rule_bytes(&result.wasm_bytes, options) {
                                    Ok(loaded_manifest) => {
                                        let internal_name = loaded_manifest.name.clone();
                                        let plugin_manifest = convert_manifest(&result.manifest);

                                        if let Err(e) = host.rename_rule(
                                            &internal_name,
                                            &rule_name,
                                            Some(plugin_manifest),
                                        ) {
                                            warn!(
                                                "Failed to register rule '{}' (loaded as '{}'): {}",
                                                rule_name, internal_name, e
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to load rule '{}': {}", rule_name, e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to load rule manifest '{}': {}",
                                    manifest_path.display(),
                                    e
                                );
                            }
                        }
                    }
                } else if let Some(github) = &detail.github {
                    warn!("GitHub rule fetching not yet implemented: {}", github);
                } else if let Some(url) = &detail.url {
                    warn!("URL rule fetching not yet implemented: {}", url);
                }
            }
        }
    }
}

pub fn get_rule_versions_from_host(host: &PluginHost) -> HashMap<String, String> {
    let mut versions = HashMap::new();

    for name in host.loaded_rules() {
        if let Some(manifest) = host.get_manifest(name) {
            versions.insert(name.to_string(), manifest.version.clone());
        }
    }

    versions
}

fn convert_manifest(
    external: &tsuzulint_manifest::ExternalRuleManifest,
) -> tsuzulint_plugin::RuleManifest {
    use tsuzulint_manifest::IsolationLevel as ExternalIsolationLevel;
    use tsuzulint_plugin::{Capability, IsolationLevel as PluginIsolationLevel, KnownLanguage};

    let isolation_level = match external.rule.isolation_level {
        ExternalIsolationLevel::Global => PluginIsolationLevel::Global,
        ExternalIsolationLevel::Block => PluginIsolationLevel::Block,
    };

    let languages: Vec<KnownLanguage> = external
        .rule
        .languages
        .iter()
        .filter_map(|s| match s.as_str() {
            "ja" => Some(KnownLanguage::Ja),
            "en" => Some(KnownLanguage::En),
            "zh" => Some(KnownLanguage::Zh),
            "ko" => Some(KnownLanguage::Ko),
            _ => None,
        })
        .collect();

    let capabilities: Vec<Capability> = external
        .rule
        .capabilities
        .iter()
        .filter_map(|s| match s.as_str() {
            "morphology" => Some(Capability::Morphology),
            "sentences" => Some(Capability::Sentences),
            _ => None,
        })
        .collect();

    tsuzulint_plugin::RuleManifest {
        name: external.rule.name.clone(),
        version: external.rule.version.clone(),
        description: external.rule.description.clone(),
        fixable: external.rule.fixable,
        node_types: external.rule.node_types.clone(),
        isolation_level,
        schema: None,
        languages,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_configured_rules_static() {
        let config = LinterConfig::new();
        let mut host = PluginHost::new();

        load_configured_rules(&config, &mut host);
        assert!(host.loaded_rules().next().is_none());
    }

    #[test]
    fn test_get_rule_versions_from_host_empty() {
        let host = PluginHost::new();
        let versions = get_rule_versions_from_host(&host);
        assert!(versions.is_empty());
    }
}
