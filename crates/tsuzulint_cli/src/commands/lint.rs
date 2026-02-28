//! Lint command implementation

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;
use tracing::{info, warn};
use tsuzulint_core::{Linter, LinterConfig, RuleDefinition, RuleDefinitionDetail};
use tsuzulint_registry::resolver::{PluginResolver, PluginSpec};

use crate::cli::{Cli, OutputFormat};
use crate::fix::{apply_fixes, output_fix_summary};
use crate::output::output_results;
use crate::utils::create_tokio_runtime;

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

    let resolver = PluginResolver::new().into_diagnostic()?;
    let runtime = create_tokio_runtime()?;

    config.rules = resolve_rules(&config.rules, &resolver, &runtime, fail_on_resolve_error)?;

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

fn resolve_rules(
    rules: &[RuleDefinition],
    resolver: &PluginResolver,
    runtime: &Runtime,
    fail_on_resolve_error: bool,
) -> Result<Vec<RuleDefinition>> {
    let mut resolved_rules = Vec::with_capacity(rules.len());

    for rule in rules {
        match extract_plugin_spec(rule) {
            Some((spec, original_alias)) => {
                match resolve_single_rule(&spec, resolver, runtime, fail_on_resolve_error) {
                    Ok(resolved) => {
                        let new_rule = RuleDefinition::Detail(RuleDefinitionDetail {
                            github: None,
                            server_url: None,
                            url: None,
                            path: Some(resolved.path),
                            r#as: original_alias.or(Some(resolved.alias)),
                        });
                        resolved_rules.push(new_rule);
                    }
                    Err(ResolveError::Skipped) => {
                        resolved_rules.push(rule.clone());
                    }
                    Err(ResolveError::Fatal(e)) => {
                        return Err(e);
                    }
                }
            }
            None => {
                resolved_rules.push(rule.clone());
            }
        }
    }

    Ok(resolved_rules)
}

struct ResolvedRule {
    path: String,
    alias: String,
}

enum ResolveError {
    Skipped,
    Fatal(miette::Report),
}

fn resolve_single_rule(
    spec: &PluginSpec,
    resolver: &PluginResolver,
    runtime: &Runtime,
    fail_on_resolve_error: bool,
) -> std::result::Result<ResolvedRule, ResolveError> {
    info!("Resolving rule: {:?}...", spec);

    let resolve_result = runtime.block_on(async { resolver.resolve(spec).await });

    match resolve_result {
        Ok(resolved) => resolved
            .manifest_path
            .to_str()
            .ok_or_else(|| {
                ResolveError::Fatal(miette::miette!(
                    "Resolved manifest path is not valid UTF-8: {:?}",
                    resolved.manifest_path
                ))
            })
            .map(|s| ResolvedRule {
                path: s.to_string(),
                alias: resolved.alias,
            }),
        Err(e) => {
            if fail_on_resolve_error {
                Err(ResolveError::Fatal(miette::miette!("{}", e)))
            } else {
                warn!("Failed to resolve rule {:?}: {}. Skipping...", spec, e);
                Err(ResolveError::Skipped)
            }
        }
    }
}

fn extract_plugin_spec(rule: &RuleDefinition) -> Option<(PluginSpec, Option<String>)> {
    match rule {
        RuleDefinition::Simple(s) => {
            let val = serde_json::Value::String(s.clone());
            let spec = PluginSpec::parse(&val).ok()?;
            if matches!(
                spec.source,
                tsuzulint_registry::resolver::PluginSource::GitHub { .. }
            ) {
                Some((spec, None))
            } else {
                None
            }
        }
        RuleDefinition::Detail(d) => {
            if let Some(gh) = &d.github {
                let (spec, alias) = super::rules::build_spec_from_detail(
                    "github",
                    gh,
                    d.server_url.as_deref(),
                    d.r#as.as_deref(),
                );
                spec.map(|s| (s, alias))
            } else if let Some(url) = &d.url {
                let (spec, alias) =
                    super::rules::build_spec_from_detail("url", url, None, d.r#as.as_deref());
                spec.map(|s| (s, alias))
            } else {
                None
            }
        }
    }
}

pub fn find_config() -> Result<LinterConfig> {
    if let Some(path) = LinterConfig::discover(".") {
        info!("Using config: {}", path.display());
        return LinterConfig::from_file(&path).into_diagnostic();
    }

    info!("No config file found, using defaults");
    Ok(LinterConfig::new())
}
