//! Plugin command implementation

use std::path::PathBuf;

use miette::{IntoDiagnostic, Result};
use tracing::{info, warn};
use tsuzulint_registry::resolver::{PluginSource, PluginSpec};

use crate::config::editor::update_config_with_plugin;

pub fn run_plugin_cache_clean() -> Result<()> {
    use tsuzulint_registry::cache::PluginCache;

    let cache = PluginCache::new().into_diagnostic()?;
    cache.clear().into_diagnostic()?;

    info!("Plugin cache cleaned");
    Ok(())
}

pub fn run_plugin_install(
    spec_str: Option<String>,
    url: Option<String>,
    alias: Option<String>,
    config_path: Option<PathBuf>,
    fail_on_resolve_error: bool,
) -> Result<()> {
    let spec = if let Some(url) = url {
        if let Some(spec_str) = spec_str {
            return Err(miette::miette!(
                "Cannot specify both a plugin spec '{}' and --url '{}'",
                spec_str,
                url
            ));
        }

        if alias.is_none() {
            return Err(miette::miette!("--as <ALIAS> is required when using --url"));
        }

        PluginSpec {
            source: PluginSource::Url(url),
            alias,
        }
    } else if let Some(s) = spec_str {
        let json_value = serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
        let mut spec = PluginSpec::parse(&json_value).into_diagnostic()?;

        if let Some(a) = alias {
            spec.alias = Some(a);
        }
        spec
    } else {
        return Err(miette::miette!("Must provide a plugin spec or --url"));
    };

    info!("Resolving plugin...");
    let resolver = tsuzulint_registry::resolver::PluginResolver::new().into_diagnostic()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;

    let resolve_result = runtime.block_on(async { resolver.resolve(&spec).await });

    let resolved = match resolve_result {
        Ok(r) => r,
        Err(e) => {
            if fail_on_resolve_error {
                return Err(e).into_diagnostic();
            } else {
                warn!(
                    "Failed to resolve plugin {:?}: {}. Aborting install.",
                    spec, e
                );
                return Ok(());
            }
        }
    };

    info!("Successfully installed: {}", resolved.manifest.rule.name);
    update_config_with_plugin(&spec, &resolved.alias, &resolved.manifest, config_path)
}
