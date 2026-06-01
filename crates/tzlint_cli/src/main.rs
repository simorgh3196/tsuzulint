//! `tzlint` — the TsuzuLint command-line binary.
//!
//! Wires the `lint` / `fix` / `init` subcommands over `tzlint_core`: config discovery, the
//! parse → archive → `Engine::lint` pipeline (with the in-memory document cache), autofix, and
//! text/JSON output via the position mapper. All file access goes through the `Host` boundary
//! (`NativeHost`), never raw `std::fs`.
//!
//! The rule set is resolved from config by [`rules::resolve_rules`]: every built-in rule
//! (`tzlint_rules::builtin_rules`) runs by default, and a `config.rules` entry can disable one.
//! Routing per-rule `options`/severity overrides into rule construction is a follow-up.

use std::process::ExitCode;

use clap::Parser;
use tzlint_core::io::NativeHost;

use crate::cli::Cli;

mod app;
mod cli;
mod output;
mod rules;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let host = NativeHost;
    // Resolve the working directory once, here at the edge, and pass it in — the orchestration
    // stays independent of process-global state (and so hermetically testable).
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let status = app::run(&cli, &host, &cwd, &mut stdout, &mut stderr);
    ExitCode::from(status.code())
}
