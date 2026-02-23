//! TsuzuLint CLI
//!
//! High-performance natural language linter written in Rust.

use std::process::ExitCode;

use clap::Parser;
use miette::Result;
use tracing::error;
use tracing_subscriber::EnvFilter;

mod cli;
mod commands;
mod config;
mod fix;
mod output;

use cli::{CacheCommands, Cli, Commands, PluginCommands, RulesCommands};

fn main() -> ExitCode {
    let cli = Cli::parse();

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
        } => commands::lint::run_lint(
            &cli,
            patterns,
            *format,
            *fix,
            *dry_run,
            *timings,
            *fail_on_resolve_error,
        ),
        Commands::Init { force } => commands::init::run_init(*force).map(|_| false),
        Commands::Rules { command } => match command {
            RulesCommands::Create { name } => commands::rules::run_create_rule(name).map(|_| false),
            RulesCommands::Add { path } => commands::rules::run_add_rule(path).map(|_| false),
        },
        Commands::Lsp => commands::lsp::run_lsp().map(|_| false),
        Commands::Plugin { command } => match command {
            PluginCommands::Cache { command } => match command {
                CacheCommands::Clean => commands::plugin::run_plugin_cache_clean().map(|_| false),
            },
            PluginCommands::Install {
                spec,
                url,
                r#as,
                fail_on_resolve_error,
            } => commands::plugin::run_plugin_install(
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
