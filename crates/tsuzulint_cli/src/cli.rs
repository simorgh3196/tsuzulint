//! CLI argument definitions

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// TsuzuLint - High-performance natural language linter
#[derive(Parser)]
#[command(name = "tzlint")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Configuration file path
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Disable caching
    #[arg(long, global = true)]
    pub no_cache: bool,
}

#[derive(Subcommand)]
pub enum Commands {
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
pub enum RulesCommands {
    /// Create a new rule project
    Create {
        /// Rule name
        name: String,
    },

    /// Add a WASM rule
    Add {
        /// Path to WASM file
        path: PathBuf,

        /// Alias name for the rule
        #[arg(long, value_name = "ALIAS")]
        r#as: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PluginCommands {
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
pub enum CacheCommands {
    /// Clean the plugin cache
    Clean,
}
