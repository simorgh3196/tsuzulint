//! TsuzuLint CLI
//!
//! High-performance natural language linter written in Rust.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use miette::{IntoDiagnostic, Result};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use jsonc_parser::ast::ObjectPropName;
use jsonc_parser::{CollectOptions, ParseOptions};
use tsuzulint_core::{
    LintResult, Linter, LinterConfig, Severity, apply_fixes_to_file, generate_sarif,
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
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Clean the plugin cache
    Clean,
}

/// Main entry point for the TsuzuLint CLI.
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

/// Runs the command specified in the CLI arguments.
fn run(cli: Cli) -> Result<bool> {
    match cli.command {
        Commands::Lint {
            ref patterns,
            ref format,
            fix,
            dry_run,
            timings,
        } => run_lint(&cli, patterns, format, fix, dry_run, timings),
        Commands::Init { force } => {
            run_init(force)?;
            Ok(false)
        }
        Commands::Rules { command } => match command {
            RulesCommands::Create { name } => {
                run_create_rule(&name)?;
                Ok(false)
            }
            RulesCommands::Add { path } => {
                run_add_rule(&path)?;
                Ok(false)
            }
        },
        Commands::Lsp => {
            run_lsp()?;
            Ok(false)
        }
        Commands::Plugin { command } => match command {
            PluginCommands::Cache { command } => match command {
                CacheCommands::Clean => {
                    run_plugin_cache_clean()?;
                    Ok(false)
                }
            },
            PluginCommands::Install { spec, url, r#as } => {
                run_plugin_install(spec, url, r#as, cli.config)?;
                Ok(false)
            }
        },
    }
}

/// Starts the Language Server Protocol (LSP) server.
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

/// Lints the files matching the given patterns.
///
/// Returns `Ok(true)` if any errors were found, `Ok(false)` otherwise.
fn run_lint(
    cli: &Cli,
    patterns: &[String],
    format: &str,
    fix: bool,
    dry_run: bool,
    timings: bool,
) -> Result<bool> {
    // Load configuration
    let mut config = if let Some(ref path) = cli.config {
        LinterConfig::from_file(path).into_diagnostic()?
    } else {
        // Try to find config file
        find_config()?
    };

    // Override timings from CLI
    if timings {
        config.timings = true;
    }

    // Disable caching if requested
    if cli.no_cache {
        config.cache = false;
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
            let has_errors = output_results(&results, format, timings_enabled)?;
            return Ok(has_errors || !failures.is_empty());
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

/// Attempts to find a configuration file in the current directory.
fn find_config() -> Result<LinterConfig> {
    let config_files = [".tsuzulint.jsonc", ".tsuzulint.json"];

    for name in config_files {
        let path = PathBuf::from(name);
        if path.exists() {
            info!("Using config: {}", name);
            return LinterConfig::from_file(&path).into_diagnostic();
        }
    }

    // Return default config if no file found
    info!("No config file found, using defaults");
    Ok(LinterConfig::new())
}

/// Outputs the linting results in the specified format.
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
            println!("{}", serde_json::to_string_pretty(&output).into_diagnostic()?);
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

/// Initializes a new TsuzuLint configuration file in the current directory.
fn run_init(force: bool) -> Result<()> {
    let config_path = PathBuf::from(".tsuzulint.jsonc");

    if config_path.exists() && !force {
        return Err(miette::miette!(
            "Config file already exists. Use --force to overwrite."
        ));
    }

    let default_config = r#"{
  "rules": [],
  "options": {},
  "cache": true
}
"#;

    std::fs::write(&config_path, default_config).into_diagnostic()?;
    info!("Created {}", config_path.display());

    Ok(())
}

/// Creates a new rule project directory with a template.
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
pub fn lint(input: String) -> FnResult<String> {{
    let request: LintRequest = serde_json::from_str(&input)?;
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
    Ok(serde_json::to_string(&response)?)
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

/// Adds a WASM rule to the configuration (stub).
fn run_add_rule(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(miette::miette!("File not found: {}", path.display()));
    }

    // For now, just verify the WASM file can be loaded
    info!("Rule added: {}", path.display());
    info!("Add the rule to your .tsuzulint.jsonc to enable it");

    Ok(())
}

/// Cleans the plugin cache directory.
fn run_plugin_cache_clean() -> Result<()> {
    use tsuzulint_registry::cache::PluginCache;

    let cache = PluginCache::new().into_diagnostic()?;
    cache.clear().into_diagnostic()?;

    info!("Plugin cache cleaned");
    Ok(())
}

/// Installs a plugin from a specification or URL.
fn run_plugin_install(
    spec_str: Option<String>,
    url: Option<String>,
    alias: Option<String>,
    config_path: Option<PathBuf>,
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
    let resolved = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(async { resolver.resolve(&spec).await })
        .into_diagnostic()?;
    info!("Successfully installed: {}", resolved.manifest.rule.name);

    // Update config
    update_config_with_plugin(&spec, &resolved.alias, &resolved.manifest, config_path)?;

    Ok(())
}

/// Updates the configuration file to include the newly installed plugin.
fn update_config_with_plugin(
    spec: &PluginSpec,
    alias: &str,
    manifest: &tsuzulint_registry::manifest::ExternalRuleManifest,
    config_path: Option<PathBuf>,
) -> Result<()> {
    let path_to_use = if let Some(path) = config_path {
        path
    } else {
        let config_path = PathBuf::from(".tsuzulint.jsonc");
        if config_path.exists() {
            config_path
        } else {
            let json_path = PathBuf::from(".tsuzulint.json");
            if json_path.exists() {
                json_path
            } else {
                // Create default .tsuzulint.jsonc
                run_init(false)?;
                config_path
            }
        }
    };

    let content = std::fs::read_to_string(&path_to_use).into_diagnostic()?;

    // Parse to AST to preserve comments
    let parse_options = ParseOptions::default();
    let collect_options = CollectOptions::default();
    let ast = jsonc_parser::parse_to_ast(&content, &collect_options, &parse_options)
        .map_err(|e| miette::miette!("Failed to parse config: {}", e))?;

    // We need to determine if we should modify "rules" and "options"
    // Since AST modification is complex, we will perform a text splice approach
    // based on AST spans.

    // First, convert to Value to check existence (logic)
    // Use jsonc-compatible parsing
    let config_value: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&content, &parse_options)
            .map_err(|e| miette::miette!("Failed to parse config config: {}", e))?
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let mut new_content = content.clone();
    let mut offset_adjustment: isize = 0;

    // Helper to insert text at position taking offset into account
    let mut insert_at = |original_pos: usize, text: &str| {
        let pos = (original_pos as isize + offset_adjustment) as usize;
        new_content.insert_str(pos, text);
        offset_adjustment += text.len() as isize;
    };

    let root_obj = ast
        .value
        .as_ref()
        .and_then(|v| v.as_object())
        .ok_or_else(|| miette::miette!("Invalid config: root must be an object"))?;

    // 1. Add to rules array
    // Construct rule definition string
    let rule_def_str = match &spec.source {
        PluginSource::GitHub { owner, repo, .. } => {
            let version = &manifest.rule.version;
            let source_str = format!("{}/{}@{}", owner, repo, version);
            let source_json = serde_json::to_string(&source_str).into_diagnostic()?;
            if let Some(a) = &spec.alias {
                let alias_json = serde_json::to_string(a).into_diagnostic()?;
                format!(r#"{{ "github": {}, "as": {} }}"#, source_json, alias_json)
            } else {
                source_json
            }
        }
        PluginSource::Url(url) => {
            let url_json = serde_json::to_string(url).into_diagnostic()?;
            let alias_json = serde_json::to_string(alias).into_diagnostic()?;
            format!(r#"{{ "url": {}, "as": {} }}"#, url_json, alias_json)
        }
        PluginSource::Path(path) => {
            let path_json = serde_json::to_string(path).into_diagnostic()?;
            let alias_json = serde_json::to_string(alias).into_diagnostic()?;
            format!(r#"{{ "path": {}, "as": {} }}"#, path_json, alias_json)
        }
    };

    // Check if rule already exists (using serde logic)
    let rule_def_json: serde_json::Value = serde_json::from_str(&rule_def_str).into_diagnostic()?;
    let needs_add_rule = if let Some(rules) = config_value.get("rules").and_then(|v| v.as_array()) {
        !rules.contains(&rule_def_json)
    } else {
        true
    };

    if needs_add_rule {
        // Check if "rules" property exists
        let rules_prop = root_obj.properties.iter().find(|p| match &p.name {
            ObjectPropName::String(s) => s.value == "rules",
            ObjectPropName::Word(w) => w.value == "rules",
        });

        if let Some(prop) = rules_prop {
            if let Some(array) = prop.value.as_array() {
                // Start of array content (after '[')
                // We append to the end
                let end_pos = array.range.end - 1; // Assuming ']' is at end
                let is_empty = array.elements.is_empty();

                let insert_str = if is_empty {
                    format!("\n    {}", rule_def_str)
                } else {
                    format!(",\n    {}", rule_def_str)
                };

                insert_at(end_pos, &insert_str);
            } else {
                return Err(miette::miette!("Invalid config: 'rules' must be an array"));
            }
        } else {
            // "rules" does not exist, insert it at start of object
            let start_pos = root_obj.range.start + 1; // After '{'
            let insert_str = format!(
                r#"
  "rules": [
    {}
  ],"#,
                rule_def_str
            );
            insert_at(start_pos, &insert_str);
        }
    }

    // 2. Add to options
    // Find "options" in AST (need to re-parse or adjust logic if we were modifying structure deeply)
    // IMPORTANT: Since we modified `new_content` but `ast` refers to `content`,
    // we must rely on `offset_adjustment` or re-parse.
    // Since `offset_adjustment` accumulates, we can continue using `ast` locations + offset.

    // Determine options text
    let default_options = if let Some(opts) = &manifest.options {
        opts.clone()
    } else {
        serde_json::Value::Bool(true)
    };

    let alias_json = serde_json::to_string(alias).into_diagnostic()?;
    let options_json = serde_json::to_string(&default_options).into_diagnostic()?;
    let options_str = format!(r#"{}: {}"#, alias_json, options_json);

    // Check logic
    let needs_add_option = config_value
        .get("options")
        .and_then(|v| v.as_object())
        .map(|o| !o.contains_key(alias))
        .unwrap_or(true);

    if needs_add_option {
        let options_prop = root_obj.properties.iter().find(|p| match &p.name {
            ObjectPropName::String(s) => s.value == "options",
            ObjectPropName::Word(w) => w.value == "options",
        });

        if let Some(prop) = options_prop {
            if let Some(obj) = prop.value.as_object() {
                let end_pos = obj.range.end - 1;
                let is_empty = obj.properties.is_empty();
                let insert_str = if is_empty {
                    format!("\n    {}", options_str)
                } else {
                    format!(",\n    {}", options_str)
                };
                insert_at(end_pos, &insert_str);
            } else {
                return Err(miette::miette!(
                    "Invalid config: 'options' must be an object"
                ));
            }
        } else {
            // "options" does not exist
            // We need to be careful about commas if we added "rules" above?
            // Actually, if "rules" existed, we are fine. If we added "rules", we added a trailing comma?
            // My logic above: `"rules": [ ... ],` (added comma).
            // So we can just append.

            // However, inserting multiple properties into root is tricky with just offsets
            // if we don't know where we inserted the previous one relative to this one.
            // "rules" strategy: insert at start (`{` + 1).
            // "options" strategy: insert at end (`}` - 1).
            // This avoids collision!

            let end_pos = root_obj.range.end - 1;

            // If we added rules, we inserted at `root_obj.range.start + 1`.
            // We are now inserting at `root_obj.range.end - 1`.
            // These are distinct locations (unless object was empty `{}`).

            // If `root_obj` has properties, we need comma.
            // If `needs_add_rule` was true AND we added rules, we definitely have properties now.
            // So if `!root_obj.properties.is_empty() || needs_add_rule`, we need a comma.

            let need_comma = !root_obj.properties.is_empty() || needs_add_rule;

            let insert_str = if need_comma {
                format!(
                    r#",
  "options": {{
    {}
  }}"#,
                    options_str
                )
            } else {
                format!(
                    r#"
  "options": {{
    {}
  }}"#,
                    options_str
                )
            };

            insert_at(end_pos, &insert_str);
        }
    }

    std::fs::write(&path_to_use, new_content).into_diagnostic()?;
    info!("Updated {}", path_to_use.display());
    Ok(())
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
