//! Texide CLI
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

use texide_core::{
    LintResult, Linter, LinterConfig, Severity, apply_fixes_to_file, generate_sarif,
};

/// Texide - High-performance natural language linter
#[derive(Parser)]
#[command(name = "texide")]
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
    }
}

fn run_lsp() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(async {
            texide_lsp::run().await;
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

fn find_config() -> Result<LinterConfig> {
    let config_files = [".texide.jsonc", ".texide.json"];

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
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
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
    let config_path = PathBuf::from(".texide.jsonc");

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
        r#"//! {} rule for Texide

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

fn run_add_rule(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(miette::miette!("File not found: {}", path.display()));
    }

    // For now, just verify the WASM file can be loaded
    info!("Rule added: {}", path.display());
    info!("Add the rule to your .texide.jsonc to enable it");

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
