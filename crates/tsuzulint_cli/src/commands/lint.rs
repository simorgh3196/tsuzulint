//! Lint command implementation

use miette::{IntoDiagnostic, Result};
use tracing::{info, warn};
use tsuzulint_core::{Linter, LinterConfig, RuleDefinition, RuleDefinitionDetail};

use crate::cli::{Cli, OutputFormat};
use crate::fix::{apply_fixes, output_fix_summary};
use crate::output::output_results;

pub fn run_lint(
    cli: &Cli,
    patterns: &[String],
    format: OutputFormat,
    fix: bool,
    dry_run: bool,
    timings: bool,
    fail_on_resolve_error: bool,
) -> Result<bool> {
    let mut config = if let Some(ref path) = cli.config {
        LinterConfig::from_file(path).into_diagnostic()?
    } else {
        find_config()?
    };

    let resolver = tsuzulint_registry::resolver::PluginResolver::new().into_diagnostic()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;

    let mut new_rules = Vec::new();
    let mut modified = false;

    for rule in &config.rules {
        let (spec, original_alias) = match rule {
            RuleDefinition::Simple(s) => {
                let val = serde_json::Value::String(s.clone());
                if let Ok(spec) = tsuzulint_registry::resolver::PluginSpec::parse(&val) {
                    if matches!(
                        spec.source,
                        tsuzulint_registry::resolver::PluginSource::GitHub { .. }
                    ) {
                        (Some(spec), None)
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            }
            RuleDefinition::Detail(d) => {
                if let Some(gh) = &d.github {
                    super::rules::build_spec_from_detail("github", gh, d.r#as.as_deref())
                } else if let Some(url) = &d.url {
                    super::rules::build_spec_from_detail("url", url, d.r#as.as_deref())
                } else {
                    (None, None)
                }
            }
        };

        if let Some(spec) = spec {
            info!("Resolving rule: {:?}...", spec);
            let resolve_result = runtime.block_on(async { resolver.resolve(&spec).await });

            let resolved = match resolve_result {
                Ok(r) => r,
                Err(e) => {
                    if fail_on_resolve_error {
                        return Err(e).into_diagnostic();
                    } else {
                        warn!("Failed to resolve rule {:?}: {}. Skipping...", spec, e);
                        new_rules.push(rule.clone());
                        continue;
                    }
                }
            };

            let path_str = resolved
                .manifest_path
                .to_str()
                .ok_or_else(|| {
                    miette::miette!(
                        "Resolved manifest path is not valid UTF-8: {:?}",
                        resolved.manifest_path
                    )
                })?
                .to_string();
            let new_rule = RuleDefinition::Detail(RuleDefinitionDetail {
                github: None,
                url: None,
                path: Some(path_str),
                r#as: original_alias.or(Some(resolved.alias)),
                sha256: Some(resolved.manifest.artifacts.sha256.clone()),
            });
            new_rules.push(new_rule);
            modified = true;
        } else {
            new_rules.push(rule.clone());
        }
    }

    if modified {
        config.rules = new_rules;
    }

    if timings {
        config.timings = true;
    }

    if cli.no_cache {
        config.cache = tsuzulint_core::CacheConfig::Boolean(false);
    }

    let timings_enabled = config.timings;

    let linter = Linter::new(config).into_diagnostic()?;

    let (results, failures) = linter.lint_patterns(patterns).into_diagnostic()?;

    if !failures.is_empty() {
        eprintln!("\n{} file(s) failed to lint:", failures.len());
        for (path, error) in &failures {
            eprintln!("  {}: {}", path.display(), error);
        }
    }

    if fix {
        let fix_summary = apply_fixes(&results, dry_run)?;
        output_fix_summary(&fix_summary, dry_run);

        if dry_run {
            return output_results(&results, format, timings_enabled);
        }

        let unfixable_errors = results
            .iter()
            .any(|r| r.diagnostics.iter().any(|d| d.fix.is_none()));
        return Ok(unfixable_errors || !failures.is_empty());
    }

    let has_errors = output_results(&results, format, timings_enabled)?;

    Ok(has_errors || !failures.is_empty())
}

pub fn find_config() -> Result<LinterConfig> {
    if let Some(path) = LinterConfig::discover(".") {
        info!("Using config: {}", path.display());
        return LinterConfig::from_file(&path).into_diagnostic();
    }

    info!("No config file found, using defaults");
    Ok(LinterConfig::new())
}
