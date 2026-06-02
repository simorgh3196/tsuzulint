//! Command-line argument definitions (clap derive).
//!
//! The surface mirrors the established `tzlint` UX: a few global options (`--config`,
//! `--verbose`, `--no-cache`) and the `lint` / `fix` / `init` / `rules` subcommands. `lint` and
//! `fix` take files, directories (recursed for Markdown), or glob patterns, and `-` for stdin
//! (see the per-argument help); `lint` renders text, JSON, or SARIF. `rules list` / `rules
//! explain` report the built-in rule set under the resolved config. Plugin and LSP subcommands
//! are intentionally out of scope here and arrive with later milestones.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// TsuzuLint — a high-performance natural-language linter for CJK.
#[derive(Parser, Debug)]
#[command(name = "tzlint", version, about, long_about = None)]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,

    /// Use this configuration file instead of discovering one by walking up from the
    /// working directory.
    #[arg(short, long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Print extra notes (which config was used, shadowed candidates) to stderr.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Disable the in-memory document cache (lint each file from scratch).
    #[arg(long, global = true)]
    pub no_cache: bool,
}

/// The available subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Lint files and report diagnostics.
    Lint {
        /// Files, directories, or globs to lint; `-` reads stdin (reported as `<stdin>`).
        ///
        /// A directory is searched recursively for `.md`/`.markdown` files (hidden entries and
        /// symlinks are skipped). Globs (`*`, `?`, `[...]`, `**`) match exactly — quote them so the
        /// shell does not expand them first.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Output format.
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },

    /// Apply autofixes to files (in place, unless `--dry-run`).
    Fix {
        /// Files, directories, or globs to fix; `-` fixes stdin and writes the result to stdout.
        ///
        /// A directory is searched recursively for `.md`/`.markdown` files (hidden entries and
        /// symlinks are skipped); globs match exactly (quote them). With `--dry-run`, changed files
        /// are reported instead of written.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Report which files would change without writing them.
        #[arg(long)]
        dry_run: bool,
    },

    /// Write a starter `.tzlintrc.json` in the working directory.
    Init {
        /// Overwrite an existing config file.
        #[arg(long)]
        force: bool,
    },

    /// Inspect the built-in rule set: which rules run, and at what severity, under the
    /// resolved config (honors `--config` / discovery).
    Rules {
        /// What to inspect (`list` or `explain`).
        #[command(subcommand)]
        command: RulesCommand,
    },
}

/// The `rules` subcommands.
#[derive(Subcommand, Debug)]
pub enum RulesCommand {
    /// List every built-in rule and whether it is enabled under the resolved config.
    List {
        /// Output format.
        #[arg(short, long, default_value = "text")]
        format: RuleListFormat,
    },

    /// Show one rule's effective state: status, severity, and config-supplied options.
    Explain {
        /// The rule id to explain (e.g. `max-ten`).
        #[arg(value_name = "RULE")]
        id: String,
    },
}

/// How `lint` renders its diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// One grep-friendly `path:line:col: severity: message [rule]` line per diagnostic.
    Text,
    /// A JSON array of `{ path, diagnostics }` objects.
    Json,
    /// A SARIF 2.1.0 log (for CI integrations such as GitHub code scanning).
    Sarif,
}

/// How `rules list` renders the rule table. (No SARIF — it is not a diagnostic stream.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RuleListFormat {
    /// Aligned `id  on|off  severity` columns plus a summary line.
    Text,
    /// A JSON array of `{ id, enabled, severity }` objects.
    Json,
}
