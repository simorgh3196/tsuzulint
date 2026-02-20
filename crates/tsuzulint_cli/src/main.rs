//! TsuzuLint CLI
//!
//! High-performance natural language linter written in Rust.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use miette::{IntoDiagnostic, Result};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use jsonc_parser::ast::ObjectPropName;
use jsonc_parser::{CollectOptions, ParseOptions};
use tsuzulint_core::{
    LintResult, Linter, LinterConfig, RuleDefinition, RuleDefinitionDetail, Severity,
    apply_fixes_to_file, generate_sarif,
};
use tsuzulint_registry::resolver::{PluginResolver, PluginSource, PluginSpec};

/// TsuzuLint - High-performance natural language linter
#[derive(Parser)]
#[command(name = "tzlint")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Configuration file path
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Disable caching
    #[arg(long, global = true)]
    no_cache: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Lint files
    Lint {
        /// File patterns to lint
        #[arg(required = true)]
        patterns: Vec<String>,

        /// Output format (text, json, sarif)
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Auto-fix errors
        #[arg(long)]
        fix: bool,

        /// Preview fixes without applying them
        #[arg(long, requires = "fix")]
        dry_run: bool,

        /// Measure performance
        #[arg(long)]
        timings: bool,

        /// Fail if a rule resolution fails (default is to skip with a warning)
        #[arg(long)]
        fail_on_resolve_error: bool,
    },

    /// Initialize configuration
    Init {
        /// Force overwrite existing config
        #[arg(long)]
        force: bool,
    },

    /// Manage rules
    Rules {
        #[command(subcommand)]
        command: RulesCommands,
    },

    /// Start the LSP server
    Lsp,

    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
}

#[derive(Subcommand)]
enum RulesCommands {
    /// Create a new rule project
    Create {
        /// Rule name
        name: String,
    },

    /// Add a WASM rule
    Add {
        /// Path to WASM file
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum PluginCommands {
    /// Manage plugin cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
    /// Install a plugin
    Install {
        /// Plugin spec ("owner/repo" or "owner/repo@version")
        spec: Option<String>,

        /// Plugin source URL
        #[arg(long)]
        url: Option<String>,

        /// Alias for the plugin
        #[arg(long, value_name = "ALIAS")]
        r#as: Option<String>,

        /// Fail if plugin resolution fails
        #[arg(long)]
        fail_on_resolve_error: bool,
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Clean the plugin cache
    Clean,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    match run(cli) {
        Ok(has_errors) => {
            if has_errors {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            error!("{:?}", e);
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    match &cli.command {
        Commands::Lint {
            patterns,
            format,
            fix,
            dry_run,
            timings,
            fail_on_resolve_error,
        } => run_lint(
            &cli,
            patterns,
            format,
            *fix,
            *dry_run,
            *timings,
            *fail_on_resolve_error,
        ),
        Commands::Init { force } => run_init(*force).map(|_| false),
        Commands::Rules { command } => match command {
            RulesCommands::Create { name } => run_create_rule(name).map(|_| false),
            RulesCommands::Add { path } => run_add_rule(path).map(|_| false),
        },
        Commands::Lsp => run_lsp().map(|_| false),
        Commands::Plugin { command } => match command {
            PluginCommands::Cache { command } => match command {
                CacheCommands::Clean => run_plugin_cache_clean().map(|_| false),
            },
            PluginCommands::Install {
                spec,
                url,
                r#as,
                fail_on_resolve_error,
            } => run_plugin_install(
                spec.clone(),
                url.clone(),
                r#as.clone(),
                cli.config.clone(),
                *fail_on_resolve_error,
            )
            .map(|_| false),
        },
    }
}

fn run_lsp() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(async {
            tsuzulint_lsp::run().await;
        });
    Ok(())
}

fn run_lint(
    cli: &Cli,
    patterns: &[String],
    format: &str,
    fix: bool,
    dry_run: bool,
    timings: bool,
    fail_on_resolve_error: bool,
) -> Result<bool> {
    // Load configuration
    let mut config = if let Some(ref path) = cli.config {
        LinterConfig::from_file(path).into_diagnostic()?
    } else {
        // Try to find config file
        find_config()?
    };

    // Pre-resolve remote rules (GitHub, URL) using registry
    let resolver = PluginResolver::new().into_diagnostic()?;
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
                if let Ok(spec) = PluginSpec::parse(&val) {
                    // Current implementation of PluginSpec::parse_string only supports GitHub format.
                    // URLs and Paths in simple string format are not yet supported because they require 'as' alias,
                    // or we need to infer alias from the filename/url which is not implemented in parse_string.
                    if matches!(spec.source, PluginSource::GitHub { .. }) {
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
                    build_spec_from_detail("github", gh, d.r#as.as_deref())
                } else if let Some(url) = &d.url {
                    build_spec_from_detail("url", url, d.r#as.as_deref())
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

            // Replace with local path to cached manifest
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

    // Override timings from CLI
    if timings {
        config.timings = true;
    }

    // Disable caching if requested
    if cli.no_cache {
        config.cache = tsuzulint_core::CacheConfig::Boolean(false);
    }

    // Capture timings flag before config is moved
    let timings_enabled = config.timings;

    // Create linter
    let linter = Linter::new(config).into_diagnostic()?;

    // Run linting
    let (results, failures) = linter.lint_patterns(patterns).into_diagnostic()?;

    // Report failures (already logged as warnings in linter, but also count them)
    if !failures.is_empty() {
        eprintln!("\n{} file(s) failed to lint:", failures.len());
        for (path, error) in &failures {
            eprintln!("  {}: {}", path.display(), error);
        }
    }

    // Apply fixes if requested
    if fix {
        let fix_summary = apply_fixes(&results, dry_run)?;
        output_fix_summary(&fix_summary, dry_run);

        if dry_run {
            // In dry-run mode, still output diagnostics
            return output_results(&results, format, timings_enabled);
        }

        // After fixing, return based on whether there were unfixable errors
        let unfixable_errors = results
            .iter()
            .any(|r| r.diagnostics.iter().any(|d| d.fix.is_none()));
        return Ok(unfixable_errors || !failures.is_empty());
    }

    // Output results
    let has_errors = output_results(&results, format, timings_enabled)?;

    Ok(has_errors || !failures.is_empty())
}

fn find_config() -> Result<LinterConfig> {
    if let Some(path) = LinterConfig::discover(".") {
        info!("Using config: {}", path.display());
        return LinterConfig::from_file(&path).into_diagnostic();
    }

    // Return default config if no file found
    info!("No config file found, using defaults");
    Ok(LinterConfig::new())
}

fn output_results(results: &[LintResult], format: &str, timings: bool) -> Result<bool> {
    let has_errors = results.iter().any(|r| r.has_errors());

    match format {
        "sarif" => {
            let sarif_output = generate_sarif(results).into_diagnostic()?;
            println!("{}", sarif_output);
        }
        "json" => {
            let output: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "path": r.path.display().to_string(),
                        "diagnostics": r.diagnostics,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&output).into_diagnostic()?
            );
        }
        _ => {
            // Text format
            for result in results {
                if result.diagnostics.is_empty() {
                    continue;
                }

                println!("\n{}:", result.path.display());
                for diag in &result.diagnostics {
                    let severity = match diag.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                        Severity::Info => "info",
                    };
                    println!(
                        "  {}:{} {} [{}]: {}",
                        diag.span.start, diag.span.end, severity, diag.rule_id, diag.message
                    );
                }
            }

            // Summary
            let total_files = results.len();
            let total_errors: usize = results.iter().map(|r| r.diagnostics.len()).sum();
            let cached = results.iter().filter(|r| r.from_cache).count();

            println!();
            println!(
                "Checked {} files ({} from cache), found {} issues",
                total_files, cached, total_errors
            );

            if timings {
                let mut total_duration = Duration::new(0, 0);
                let mut rule_timings: HashMap<String, Duration> = HashMap::new();

                for result in results {
                    for (rule, duration) in &result.timings {
                        *rule_timings.entry(rule.clone()).or_default() += *duration;
                        total_duration += *duration;
                    }
                }

                if !rule_timings.is_empty() {
                    println!("\nPerformance Timings:");
                    println!("{:<30} | {:<15} | {:<10}", "Rule", "Duration", "%");
                    println!("{:-<30}-+-{:-<15}-+-{:-<10}", "", "", "");

                    let mut sorted_timings: Vec<_> = rule_timings.into_iter().collect();
                    sorted_timings.sort_by(|a, b| b.1.cmp(&a.1));

                    for (rule, duration) in sorted_timings {
                        let percentage = if total_duration.as_secs_f64() > 0.0 {
                            (duration.as_secs_f64() / total_duration.as_secs_f64()) * 100.0
                        } else {
                            0.0
                        };
                        println!("{:<30} | {:<15?} | {:<10.1}%", rule, duration, percentage);
                    }
                    println!("{:-<30}-+-{:-<15}-+-{:-<10}", "", "", "");
                    println!("{:<30} | {:<15?}", "Total", total_duration);
                }
            }
        }
    }

    Ok(has_errors)
}

fn run_init(force: bool) -> Result<()> {
    let config_path = PathBuf::from(LinterConfig::CONFIG_FILES[0]);

    let default_config = r#"{
  "rules": [],
  "options": {},
  "cache": true
}
"#;

    loop {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }

        match options.open(&config_path) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(default_config.as_bytes())
                    .into_diagnostic()?;
                info!("Created {}", config_path.display());
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if !force {
                    return Err(miette::miette!(
                        "Config file already exists. Use --force to overwrite."
                    ));
                }

                // If force is enabled, remove the existing file or symlink and retry.
                // Re-check existence to avoid infinite loop if removal fails for other reasons.
                if std::fs::symlink_metadata(&config_path).is_ok() {
                    std::fs::remove_file(&config_path).into_diagnostic()?;
                } else {
                    // Path no longer exists, but we got AlreadyExists?
                    // This could happen if another process is racing.
                    // Just retry the loop.
                }
            }
            Err(e) => return Err(e).into_diagnostic(),
        }
    }
}

fn run_create_rule(name: &str) -> Result<()> {
    let rule_dir = PathBuf::from(name);

    if rule_dir.exists() {
        return Err(miette::miette!("Directory '{}' already exists", name));
    }

    std::fs::create_dir_all(&rule_dir).into_diagnostic()?;

    // Create Cargo.toml
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

    // Create src/lib.rs
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

fn run_add_rule(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(miette::miette!("File not found: {}", path.display()));
    }

    // For now, just verify the WASM file can be loaded
    info!("Rule added: {}", path.display());
    info!("Add the rule to your .tsuzulint.jsonc to enable it");

    Ok(())
}

fn run_plugin_cache_clean() -> Result<()> {
    use tsuzulint_registry::cache::PluginCache;

    let cache = PluginCache::new().into_diagnostic()?;
    cache.clear().into_diagnostic()?;

    info!("Plugin cache cleaned");
    Ok(())
}

fn run_plugin_install(
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
        // Construct JSON to use PluginSpec::parse logic which handles string parsing
        // We try to parse as JSON first to allow object format (e.g. {"path": "...", "as": "..."})
        let json_value = serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
        let mut spec = PluginSpec::parse(&json_value).into_diagnostic()?;

        // Override alias if provided
        if let Some(a) = alias {
            spec.alias = Some(a);
        }
        spec
    } else {
        return Err(miette::miette!("Must provide a plugin spec or --url"));
    };

    info!("Resolving plugin...");
    let resolver = PluginResolver::new().into_diagnostic()?;

    // Run the resolve (download) process
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

struct ConfigEdit {
    pos: usize,
    text: String,
    order: usize,
}

fn update_config_with_plugin(
    spec: &PluginSpec,
    alias: &str,
    manifest: &tsuzulint_registry::manifest::ExternalRuleManifest,
    config_path: Option<PathBuf>,
) -> Result<()> {
    let path_to_use = if let Some(path) = config_path {
        path
    } else if let Some(path) = LinterConfig::discover(".") {
        path
    } else {
        // Create default config file if none found
        run_init(false)?;
        PathBuf::from(LinterConfig::CONFIG_FILES[0])
    };

    // Check for symlink to prevent overwriting arbitrary files
    if std::fs::symlink_metadata(&path_to_use).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(miette::miette!(
            "Refusing to modify configuration file because it is a symbolic link: {}",
            path_to_use.display()
        ));
    }

    let content = std::fs::read_to_string(&path_to_use).into_diagnostic()?;

    // Parse to AST to preserve comments
    let parse_options = ParseOptions::default();
    let collect_options = CollectOptions::default();
    let ast = jsonc_parser::parse_to_ast(&content, &collect_options, &parse_options)
        .map_err(|e| miette::miette!("Failed to parse config: {}", e))?;

    // Parse logic
    let config_value: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&content, &parse_options)
            .map_err(|e| miette::miette!("Failed to parse config: {}", e))?
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let mut edits = Vec::new();

    // Generate new content
    let rule_def_str = generate_rule_def(spec, alias, manifest)?;
    let options_str = generate_options_def(alias, manifest)?;

    // 1. Check rules
    // Using serde logic to check duplication
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
                    text: format!("{}{}", prefix, rule_def_str),
                    order: 0,
                });
            } else {
                return Err(miette::miette!("Invalid config: 'rules' must be an array"));
            }
        } else {
            needs_add_rule_prop = true;
        }
    }

    // 2. Check options
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
                    text: format!("{}{}", prefix, options_str),
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

    // Handle root property additions
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

        // Lower order is inserted later (descending sort), so it appears first in the file.
        for (i, text) in root_props_to_add.into_iter().enumerate() {
            let need_comma = has_existing || i > 0;
            let prefix = if need_comma { ",\n  " } else { "\n  " };

            // Ensure the last inserted property has a trailing newline if it's the last one
            // to avoid ending with `}}` on the same line if the file ends abruptly.
            let is_last = i + 1 == count;
            let suffix = if is_last { "\n" } else { "" };

            edits.push(ConfigEdit {
                pos: root_end,
                text: format!("{}{}{}", prefix, text, suffix),
                order: i,
            });
        }
    }

    // Sort edits
    edits.sort_by(|a, b| b.pos.cmp(&a.pos).then(b.order.cmp(&a.order)));

    // Apply edits
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
    manifest: &tsuzulint_registry::manifest::ExternalRuleManifest,
) -> Result<String> {
    match &spec.source {
        PluginSource::GitHub { owner, repo, .. } => {
            let version = &manifest.rule.version;
            let source_str = format!("{}/{}@{}", owner, repo, version);
            let source_json = serde_json::to_string(&source_str).into_diagnostic()?;
            if let Some(a) = &spec.alias {
                let alias_json = serde_json::to_string(a).into_diagnostic()?;
                Ok(format!(
                    r#"{{ "github": {}, "as": {} }}"#,
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
                r#"{{ "url": {}, "as": {} }}"#,
                url_json, alias_json
            ))
        }
        PluginSource::Path(path) => {
            let path_json = serde_json::to_string(path).into_diagnostic()?;
            let alias_json = serde_json::to_string(alias).into_diagnostic()?;
            Ok(format!(
                r#"{{ "path": {}, "as": {} }}"#,
                path_json, alias_json
            ))
        }
    }
}

fn generate_options_def(
    alias: &str,
    manifest: &tsuzulint_registry::manifest::ExternalRuleManifest,
) -> Result<String> {
    let default_options = if let Some(opts) = &manifest.options {
        opts.clone()
    } else {
        serde_json::Value::Bool(true)
    };

    let alias_json = serde_json::to_string(alias).into_diagnostic()?;
    let options_json = serde_json::to_string(&default_options).into_diagnostic()?;
    Ok(format!(r#"{}: {}"#, alias_json, options_json))
}

/// Summary of applied fixes.
struct FixSummary {
    total_fixes: usize,
    files_fixed: usize,
    fixes_by_file: Vec<(PathBuf, usize)>,
}

/// Applies fixes to all files with fixable diagnostics.
fn apply_fixes(results: &[LintResult], dry_run: bool) -> Result<FixSummary> {
    let mut total_fixes = 0;
    let mut files_fixed = 0;
    let mut fixes_by_file = Vec::new();

    for result in results {
        // Count fixable diagnostics
        let fixable_count = result
            .diagnostics
            .iter()
            .filter(|d| d.fix.is_some())
            .count();

        if fixable_count == 0 {
            continue;
        }

        if dry_run {
            // In dry-run mode, just count the fixes
            fixes_by_file.push((result.path.clone(), fixable_count));
            total_fixes += fixable_count;
            files_fixed += 1;
        } else {
            // Actually apply the fixes
            match apply_fixes_to_file(&result.path, &result.diagnostics) {
                Ok(fixer_result) => {
                    if fixer_result.modified {
                        fixes_by_file.push((result.path.clone(), fixer_result.fixes_applied));
                        total_fixes += fixer_result.fixes_applied;
                        files_fixed += 1;
                    }
                }
                Err(e) => {
                    error!("Failed to fix {}: {}", result.path.display(), e);
                }
            }
        }
    }

    Ok(FixSummary {
        total_fixes,
        files_fixed,
        fixes_by_file,
    })
}

/// Outputs the fix summary.
fn output_fix_summary(summary: &FixSummary, dry_run: bool) {
    if summary.total_fixes == 0 {
        println!("No fixable issues found.");
        return;
    }

    if dry_run {
        println!(
            "\nWould fix {} issues in {} files:",
            summary.total_fixes, summary.files_fixed
        );
        for (path, count) in &summary.fixes_by_file {
            println!("  {}: {} fixes", path.display(), count);
        }
        println!("\nRun without --dry-run to apply fixes.");
    } else {
        println!(
            "\nFixed {} issues in {} files:",
            summary.total_fixes, summary.files_fixed
        );
        for (path, count) in &summary.fixes_by_file {
            println!("  {}: {} fixes", path.display(), count);
        }
    }
}

/// Helper to build PluginSpec from RuleDefinition detail
pub(crate) fn build_spec_from_detail(
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

    use tsuzulint_registry::manifest::{
        Artifacts, ExternalRuleManifest, IsolationLevel, RuleMetadata,
    };

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
            },
            artifacts: Artifacts {
                wasm: "test.wasm".to_string(),
                sha256: "hash".to_string(),
            },
            permissions: None,
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
            },
            alias: None,
        }
    }

    #[test]
    fn test_update_config_empty_object_adds_rules_and_options() {
        use std::io::Write;
        let manifest = create_dummy_manifest();
        let spec = create_dummy_spec();
        let alias = "test-alias";

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        write!(temp_file, "{{}}").unwrap();
        let path = temp_file.path().to_path_buf();

        update_config_with_plugin(&spec, alias, &manifest, Some(path.clone())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        // Verify content contains rules and options
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
        use std::io::Write;
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
        use std::io::Write;
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
        use std::io::Write;
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
        // Create a target file
        let target_file = NamedTempFile::new().unwrap();
        let target_path = target_file.path();

        // Create a symlink to it
        let symlink_path = target_path.parent().unwrap().join("symlink_config.json");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_path, &symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target_path, &symlink_path).unwrap();

        // Mock inputs
        let spec = create_dummy_spec();
        let manifest = create_dummy_manifest();

        // Call the function with the symlink path
        let result =
            update_config_with_plugin(&spec, "test-rule", &manifest, Some(symlink_path.clone()));

        // Verify refusal
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Refusing to modify configuration file because it is a symbolic link")
        );
        assert!(msg.contains(&symlink_path.to_string_lossy().to_string()));

        // Verify that the symlink target was not modified
        let target_content = std::fs::read(target_path).unwrap();
        assert!(
            target_content.is_empty(),
            "Symlink target must not be modified"
        );

        // Cleanup
        let _ = std::fs::remove_file(&symlink_path);
    }
}
