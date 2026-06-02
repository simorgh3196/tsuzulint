//! Command orchestration: resolve the config, run the pipeline over each file, and report.
//!
//! Every filesystem touch goes through the injected [`Host`], and the working directory is
//! passed in rather than read from the process, so the whole flow is unit-testable against an
//! in-memory host (see the tests) and reused unchanged by any embedder. Each command returns an
//! [`ExitStatus`]; [`run`] turns an unexpected error into a message on stderr and the `Error`
//! status.
//!
//! Output policy: writes to **stdout** are user-facing results, so their errors propagate (a
//! failed result write becomes `Error`). Writes to **stderr** (errors, notes) are best-effort
//! `let _ = writeln!(…)`: a failed stderr write must not abort the run or change the exit code,
//! which already signals the outcome.
//!
//! Inputs are expanded by [`expand`] (globs / directories / the `-` stdin sentinel). Stdin is
//! linted/fixed in memory under the label `<stdin>` and is never cached. `fix -` writes the fixed
//! document to **stdout** (a pass-through filter), so its progress/summary move to stderr to keep
//! stdout a pure artifact; file fixes keep their progress on stdout as before.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tzlint_ast::{access, to_archive};
use tzlint_core::io::{MAX_CONFIG, MAX_FILE};
use tzlint_core::{
    Config, ConfigFormat, DocumentCache, Engine, Host, discover, lint_cached, parse,
};
use tzlint_pdk::{Diagnostic, Rule};

use crate::cli::{Cli, Command, OutputFormat, RuleListFormat, RulesCommand};
use crate::expand;
use crate::output::{self, FileReport};
use crate::rules::{resolve_rules, rule_info, rule_infos, unknown_rule_ids};

/// The path label used for diagnostics linted from standard input.
const STDIN_LABEL: &str = "<stdin>";

/// The three standard streams, bundled so the orchestration threads I/O as one value (and stays
/// under the argument-count lint). `stdin` feeds the `-` source; `stdout` carries results, so its
/// write errors propagate; `stderr` carries notes/errors and is best-effort.
pub struct Streams<'a> {
    /// Standard input — the source for the `-` argument.
    pub stdin: &'a mut dyn Read,
    /// Standard output — user-facing results (lint output, the fixed stdin document).
    pub stdout: &'a mut dyn Write,
    /// Standard error — notes, warnings, and errors.
    pub stderr: &'a mut dyn Write,
}

/// The process exit status, mapped to a code by [`ExitStatus::code`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    /// No diagnostics and no errors.
    Clean,
    /// Linting completed but reported one or more diagnostics.
    Findings,
    /// An operational error occurred (bad config, unreadable file, write failure, …).
    Error,
}

impl ExitStatus {
    /// The conventional process exit code. `Error` (2) takes precedence over `Findings` (1)
    /// over `Clean` (0).
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            ExitStatus::Clean => 0,
            ExitStatus::Findings => 1,
            ExitStatus::Error => 2,
        }
    }
}

/// The started config used when init writes a new file. Minimal but valid: an empty rule map
/// (so nothing is overridden) and the document language. Plain `.json` (strict), so no comments.
const STARTER_CONFIG: &str = "{\n  \"language\": \"ja\",\n  \"rules\": {}\n}\n";

/// Dispatch the parsed [`Cli`] to its subcommand, writing output to `stdout`/`stderr`.
///
/// A subcommand's unexpected error is rendered as `error: …` on stderr and becomes
/// [`ExitStatus::Error`]; per-file problems are reported inline and do not abort the run.
pub fn run(cli: &Cli, host: &dyn Host, cwd: &Path, streams: &mut Streams) -> ExitStatus {
    let result = match &cli.command {
        Command::Lint { paths, format } => lint(cli, host, cwd, paths, *format, streams),
        Command::Fix { paths, dry_run } => fix(cli, host, cwd, paths, *dry_run, streams),
        Command::Init { force } => init(host, cwd, *force, streams.stdout, streams.stderr),
        Command::Rules { command } => rules_cmd(cli, host, cwd, command, streams),
    };
    match result {
        Ok(status) => status,
        Err(message) => {
            // stderr is best-effort (see the module's output policy); the `Error` status below
            // is the authoritative signal regardless of whether this message lands.
            let _ = writeln!(streams.stderr, "error: {message}");
            ExitStatus::Error
        }
    }
}

/// `lint`: read, lint, and render each file; exit `Findings` if any diagnostics, `Error` if any
/// file could not be read or linted.
fn lint(
    cli: &Cli,
    host: &dyn Host,
    cwd: &Path,
    paths: &[PathBuf],
    format: OutputFormat,
    streams: &mut Streams,
) -> Result<ExitStatus, String> {
    // Reborrow the three streams as locals (disjoint fields, so all three coexist) — the body then
    // reads exactly as it did when they were separate parameters.
    let stdin: &mut dyn Read = &mut *streams.stdin;
    let stdout: &mut dyn Write = &mut *streams.stdout;
    let stderr: &mut dyn Write = &mut *streams.stderr;
    let config = load_config(cli, host, cwd, stderr)?;
    let rules = resolve_rules(&config);
    note_if_no_rules(&rules, stderr);
    note_unknown_rules(&config, stderr);
    let rule_refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();

    // Resolve the PATH arguments into concrete files (globs / directories expanded, sorted and
    // de-duplicated) plus an optional stdin source; surface any discovery notes on stderr.
    let inputs = expand::expand(host, cwd, paths);
    note_expansion(&inputs.notes, stderr);

    // Persistent cache: load the on-disk file (best-effort) so a repeat run on unchanged content
    // skips parse+lint, and write it back afterwards. `--no-cache` skips both ends. (stdin is
    // never cached — it has no stable path key — so it always takes the direct path below.)
    let cache_path = cwd.join(".tzlintcache");
    let mut cache = (!cli.no_cache).then(|| DocumentCache::load(host, &cache_path));
    let mut reports = Vec::with_capacity(inputs.files.len() + usize::from(inputs.stdin));
    let mut had_error = false;
    // stdin is reported first so the work order is fixed and the summary count is stable.
    if inputs.stdin {
        match read_stdin(stdin).and_then(|source| {
            let diagnostics = lint_direct(&source, &rule_refs)?;
            Ok(FileReport {
                path: PathBuf::from(STDIN_LABEL),
                source,
                diagnostics,
            })
        }) {
            Ok(report) => reports.push(report),
            Err(message) => {
                let _ = writeln!(stderr, "error: {STDIN_LABEL}: {message}");
                had_error = true;
            }
        }
    }
    for path in &inputs.files {
        match read_and_lint(host, cache.as_mut(), path, &config, &rule_refs) {
            Ok(report) => reports.push(report),
            Err(message) => {
                let _ = writeln!(stderr, "error: {}: {message}", path.display());
                had_error = true;
            }
        }
    }

    // A cache write failure must not fail the run; the results are already correct.
    if let Some(cache) = &cache
        && let Err(e) = cache.save(host, &cache_path)
    {
        let _ = writeln!(
            stderr,
            "warning: could not write {}: {e}",
            cache_path.display()
        );
    }

    match format {
        OutputFormat::Text => output::render_text(stdout, &reports).map_err(|e| e.to_string())?,
        OutputFormat::Json => output::render_json(stdout, &reports).map_err(|e| e.to_string())?,
        OutputFormat::Sarif => {
            output::render_sarif(stdout, &reports).map_err(|e| e.to_string())?;
        }
    }

    let has_findings = reports.iter().any(|report| !report.diagnostics.is_empty());
    Ok(lint_exit_status(had_error, has_findings))
}

/// Combine the per-file outcome flags into the lint exit status, with `Error` taking precedence
/// over `Findings` over `Clean`.
fn lint_exit_status(had_error: bool, has_findings: bool) -> ExitStatus {
    if had_error {
        ExitStatus::Error
    } else if has_findings {
        ExitStatus::Findings
    } else {
        ExitStatus::Clean
    }
}

/// Read `path` through the host and lint it, via the cache when enabled or the direct
/// parse→archive→lint bridge otherwise.
fn read_and_lint(
    host: &dyn Host,
    cache: Option<&mut DocumentCache>,
    path: &Path,
    config: &Config,
    rules: &[&dyn Rule],
) -> Result<FileReport, String> {
    let source = host
        .read_to_string(path, MAX_FILE)
        .map_err(|e| e.to_string())?;
    let diagnostics = match cache {
        Some(cache) => lint_cached(cache, &source, config, rules).map_err(|e| e.to_string())?,
        None => lint_direct(&source, rules)?,
    };
    Ok(FileReport {
        path: path.to_path_buf(),
        source,
        diagnostics,
    })
}

/// The no-cache path: parse → archive → access → [`Engine::lint`], surfacing parse/archive
/// failures as a message (mirrors what [`lint_cached`] reports on the cached path).
fn lint_direct(source: &str, rules: &[&dyn Rule]) -> Result<Vec<Diagnostic>, String> {
    let ast = parse(source).map_err(|e| e.to_string())?;
    let bytes = to_archive(&ast).map_err(|e| format!("archive failed: {e}"))?;
    let archived = access(&bytes).map_err(|e| format!("archive failed: {e}"))?;
    Ok(Engine::lint(archived, rules))
}

/// `fix`: lint-and-fix each file to a fixpoint; write changed files in place (or just report
/// them under `--dry-run`).
fn fix(
    cli: &Cli,
    host: &dyn Host,
    cwd: &Path,
    paths: &[PathBuf],
    dry_run: bool,
    streams: &mut Streams,
) -> Result<ExitStatus, String> {
    // Reborrow the three streams as locals (disjoint fields, so all three coexist).
    let stdin: &mut dyn Read = &mut *streams.stdin;
    let stdout: &mut dyn Write = &mut *streams.stdout;
    let stderr: &mut dyn Write = &mut *streams.stderr;
    let config = load_config(cli, host, cwd, stderr)?;
    let rules = resolve_rules(&config);
    note_if_no_rules(&rules, stderr);
    note_unknown_rules(&config, stderr);
    let rule_refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();

    let inputs = expand::expand(host, cwd, paths);
    note_expansion(&inputs.notes, stderr);

    // When stdin is a target, stdout carries the fixed stdin document (a pass-through filter), so
    // ALL progress/summary moves to stderr to keep stdout pure data. Without stdin, progress and
    // the summary stay on stdout exactly as before.
    let progress_to_stderr = inputs.stdin;
    let mut changed = 0usize;
    let mut had_error = false;

    // stdin first: fix the buffer and write the result to stdout (even when unchanged, so
    // `cat x.md | tzlint fix -` is a safe pass-through). `--dry-run` emits no document.
    if inputs.stdin {
        match read_stdin(stdin) {
            Ok(source) => {
                let fixed = tzlint_core::fix(&source, &rule_refs);
                let did_change = fixed != source;
                if dry_run {
                    if did_change {
                        let _ = writeln!(stderr, "would fix {STDIN_LABEL}");
                    }
                } else {
                    write!(stdout, "{fixed}").map_err(|e| e.to_string())?;
                    let _ = writeln!(
                        stderr,
                        "{} {STDIN_LABEL}",
                        if did_change {
                            "fixed"
                        } else {
                            "no changes for"
                        }
                    );
                }
                if did_change {
                    changed += 1;
                }
            }
            Err(message) => {
                let _ = writeln!(stderr, "error: {STDIN_LABEL}: {message}");
                had_error = true;
            }
        }
    }

    for path in &inputs.files {
        let source = match host.read_to_string(path, MAX_FILE) {
            Ok(source) => source,
            Err(e) => {
                let _ = writeln!(stderr, "error: {}: {e}", path.display());
                had_error = true;
                continue;
            }
        };
        // `fix` parses internally and returns the source unchanged on a parse failure, so the
        // only failure to handle here is the write below.
        let fixed = tzlint_core::fix(&source, &rule_refs);
        if fixed == source {
            continue;
        }
        if dry_run {
            emit_progress(
                progress_to_stderr,
                stdout,
                stderr,
                &format!("would fix {}", path.display()),
            )?;
        } else if let Err(e) = host.write_atomic(path, fixed.as_bytes()) {
            let _ = writeln!(stderr, "error: {}: {e}", path.display());
            had_error = true;
            continue;
        } else {
            emit_progress(
                progress_to_stderr,
                stdout,
                stderr,
                &format!("fixed {}", path.display()),
            )?;
        }
        changed += 1;
    }

    let verb = if dry_run {
        "would be changed"
    } else {
        "changed"
    };
    emit_progress(
        progress_to_stderr,
        stdout,
        stderr,
        &format!("{changed} file(s) {verb}"),
    )?;
    Ok(if had_error {
        ExitStatus::Error
    } else {
        ExitStatus::Clean
    })
}

/// Write a progress/summary line to stdout, or to stderr (best-effort) when stdout is reserved
/// for a fixed stdin document. stdout errors propagate (a failed result write becomes `Error`).
fn emit_progress(
    to_stderr: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    message: &str,
) -> Result<(), String> {
    if to_stderr {
        let _ = writeln!(stderr, "{message}");
        Ok(())
    } else {
        writeln!(stdout, "{message}").map_err(|e| e.to_string())
    }
}

/// Read standard input into a UTF-8 string, capped at [`MAX_FILE`] bytes (mirroring
/// [`Host::read_to_string`]) so a runaway pipe cannot exhaust memory.
fn read_stdin(stdin: &mut dyn Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    // Read one past the cap so "exactly at the limit" is distinguishable from "over it".
    stdin
        .take((MAX_FILE as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE {
        return Err(format!("input exceeds the {MAX_FILE}-byte limit"));
    }
    String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {e}"))
}

/// Surface each input-expansion note (unreadable subdirectory, depth / file caps, an invalid
/// glob) as a best-effort `warning:` on stderr.
fn note_expansion(notes: &[String], stderr: &mut dyn Write) {
    for note in notes {
        let _ = writeln!(stderr, "warning: {note}");
    }
}

/// `init`: write [`STARTER_CONFIG`] to `.tzlintrc.json` in the working directory, refusing to
/// clobber an existing file unless `force`.
fn init(
    host: &dyn Host,
    cwd: &Path,
    force: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<ExitStatus, String> {
    let path = cwd.join(".tzlintrc.json");
    // Best-effort guard: there is a small TOCTOU window between this check and the write (the
    // `Host` write does not expose create-new semantics yet). Acceptable for `init`.
    if host.exists(&path) && !force {
        let _ = writeln!(
            stderr,
            "error: {} already exists (use --force to overwrite)",
            path.display()
        );
        return Ok(ExitStatus::Error);
    }
    host.write_atomic(&path, STARTER_CONFIG.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(stdout, "created {}", path.display()).map_err(|e| e.to_string())?;
    Ok(ExitStatus::Clean)
}

/// `rules`: report the built-in rule set under the resolved config — the full list or one rule's
/// details. Reads the same config as `lint`/`fix` (so it answers "what runs for this project?")
/// and warns about unknown config rule ids. A clean run exits `Clean`; an unknown `explain` id
/// exits `Error`.
fn rules_cmd(
    cli: &Cli,
    host: &dyn Host,
    cwd: &Path,
    command: &RulesCommand,
    streams: &mut Streams,
) -> Result<ExitStatus, String> {
    let stdout: &mut dyn Write = &mut *streams.stdout;
    let stderr: &mut dyn Write = &mut *streams.stderr;
    let config = load_config(cli, host, cwd, stderr)?;
    note_unknown_rules(&config, stderr);
    match command {
        RulesCommand::List { format } => {
            let infos = rule_infos(&config);
            match format {
                RuleListFormat::Text => {
                    output::render_rule_list_text(stdout, &infos).map_err(|e| e.to_string())?;
                }
                RuleListFormat::Json => {
                    output::render_rule_list_json(stdout, &infos).map_err(|e| e.to_string())?;
                }
            }
            Ok(ExitStatus::Clean)
        }
        RulesCommand::Explain { id } => match rule_info(&config, id) {
            Some(info) => {
                output::render_rule_explain(stdout, &info).map_err(|e| e.to_string())?;
                Ok(ExitStatus::Clean)
            }
            None => {
                // Not a built-in rule (likely a typo). Name it and exit `Error` rather than
                // printing a misleading "all defaults" block for a rule that does not exist.
                let _ = writeln!(stderr, "error: unknown rule '{id}'");
                Ok(ExitStatus::Error)
            }
        },
    }
}

/// Resolve the configuration: read `--config` if given (format inferred from its name, JSONC
/// otherwise), else walk up from the working directory via [`discover`], else the default.
fn load_config(
    cli: &Cli,
    host: &dyn Host,
    cwd: &Path,
    stderr: &mut dyn Write,
) -> Result<Config, String> {
    if let Some(path) = &cli.config {
        // Infer the format from the name and fail loudly on an unrecognized one, rather than
        // silently assuming JSONC and surfacing a confusing parse error downstream.
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            format!(
                "{}: unrecognized config format (expected .json, .jsonc, .yaml, .yml, or .tzlintrc)",
                path.display()
            )
        })?;
        let text = host
            .read_to_string(path, MAX_CONFIG)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let config =
            Config::parse(&text, format).map_err(|e| format!("{}: {e}", path.display()))?;
        if cli.verbose {
            let _ = writeln!(stderr, "note: using config {}", path.display());
        }
        return Ok(config);
    }

    match discover(host, cwd) {
        Ok(Some(found)) => {
            if cli.verbose {
                let _ = writeln!(stderr, "note: using config {}", found.path.display());
                for warning in &found.warnings {
                    let _ = writeln!(stderr, "note: {warning}");
                }
            }
            Ok(found.config)
        }
        Ok(None) => {
            if cli.verbose {
                let _ = writeln!(stderr, "note: no config file found; using defaults");
            }
            Ok(Config::default())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Print a one-line note to stderr when the resolved rule set is empty (every built-in rule was
/// turned off by config), so an empty result is not mistaken for "your text is clean".
fn note_if_no_rules(rules: &[Box<dyn Rule>], stderr: &mut dyn Write) {
    if rules.is_empty() {
        let _ = writeln!(
            stderr,
            "note: every built-in rule is disabled by config; the pipeline runs but reports nothing"
        );
    }
}

/// Print a stderr note for each `config.rules` id that is not a built-in rule (likely a typo),
/// so a misspelled setting is not silently ignored. Shared by `lint` and `fix`.
fn note_unknown_rules(config: &Config, stderr: &mut dyn Write) {
    for id in unknown_rule_ids(config) {
        let _ = writeln!(
            stderr,
            "note: config references unknown rule '{id}' (ignored)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    use tzlint_core::{DirEntry, EntryKind, IoError};

    use crate::cli::Command;

    /// An in-memory [`Host`]: only registered paths exist; writes are recorded so tests can
    /// assert on them. [`MockHost::put`] also synthesizes the directory listing for every ancestor
    /// of the file, so `list_dir` (and thus glob / directory expansion) works hermetically.
    struct MockHost {
        files: RefCell<HashMap<PathBuf, String>>,
        dirs: RefCell<HashMap<PathBuf, Vec<DirEntry>>>,
    }

    impl MockHost {
        fn new() -> Self {
            MockHost {
                files: RefCell::new(HashMap::new()),
                dirs: RefCell::new(HashMap::new()),
            }
        }
        fn with(path: &str, contents: &str) -> Self {
            let host = MockHost::new();
            host.put(path, contents);
            host
        }
        fn put(&self, path: &str, contents: &str) {
            let leaf = PathBuf::from(path);
            self.files
                .borrow_mut()
                .insert(leaf.clone(), contents.to_string());
            self.register_dir_entries(&leaf);
        }
        /// Register each (parent -> child) hop of `leaf` in the `dirs` index so the file is
        /// discoverable via `list_dir`. Shared by `put` and `write_atomic` so a file written
        /// through either path is visible to a later walk — matching `NativeHost`, where any
        /// created file shows up in `read_dir`.
        fn register_dir_entries(&self, leaf: &Path) {
            let mut dirs = self.dirs.borrow_mut();
            let mut child = leaf.to_path_buf();
            while let Some(parent) = child.parent() {
                if parent.as_os_str().is_empty() {
                    break;
                }
                if let Some(name) = child.file_name() {
                    let name = name.to_string_lossy().into_owned();
                    let kind = if child == leaf {
                        EntryKind::File
                    } else {
                        EntryKind::Dir
                    };
                    let children = dirs.entry(parent.to_path_buf()).or_default();
                    if !children.iter().any(|e| e.name == name) {
                        children.push(DirEntry { name, kind });
                    }
                }
                child = parent.to_path_buf();
            }
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(&PathBuf::from(path)).cloned()
        }
    }

    impl Host for MockHost {
        fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
            match self.files.borrow().get(path) {
                Some(content) if content.len() > limit => Err(IoError::TooLarge { limit }),
                Some(content) => Ok(content.clone()),
                None => Err(IoError::NotFound),
            }
        }
        fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
            let text =
                String::from_utf8(contents.to_vec()).map_err(|e| IoError::Other(e.to_string()))?;
            self.files.borrow_mut().insert(path.to_path_buf(), text);
            // Keep the `dirs` index consistent with a write, so a written file is discoverable by
            // a later `list_dir` — as it would be under `NativeHost`.
            self.register_dir_entries(path);
            Ok(())
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
        }
        fn list_dir(&self, dir: &Path) -> Result<Vec<DirEntry>, IoError> {
            self.dirs
                .borrow()
                .get(dir)
                .cloned()
                .ok_or(IoError::NotFound)
        }
    }

    fn cli(command: Command) -> Cli {
        Cli {
            command,
            config: None,
            verbose: false,
            no_cache: false,
        }
    }

    fn lint_cli(path: &str, format: OutputFormat) -> Cli {
        cli(Command::Lint {
            paths: vec![PathBuf::from(path)],
            format,
        })
    }

    fn rules_list_cli(format: RuleListFormat) -> Cli {
        cli(Command::Rules {
            command: RulesCommand::List { format },
        })
    }

    fn rules_explain_cli(id: &str) -> Cli {
        cli(Command::Rules {
            command: RulesCommand::Explain { id: id.to_string() },
        })
    }

    /// A fixed, absolute working directory for hermetic discovery/init tests (the orchestration
    /// never touches the real process cwd — it is injected).
    const TEST_CWD: &str = "/work";

    fn run_capture(cli: &Cli, host: &dyn Host) -> (ExitStatus, String, String) {
        run_capture_stdin(cli, host, b"")
    }

    /// Like [`run_capture`] but feeds `stdin` to the command (for `-` tests).
    fn run_capture_stdin(cli: &Cli, host: &dyn Host, stdin: &[u8]) -> (ExitStatus, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut input = std::io::Cursor::new(stdin.to_vec());
        let status = {
            let mut streams = Streams {
                stdin: &mut input,
                stdout: &mut out,
                stderr: &mut err,
            };
            run(cli, host, Path::new(TEST_CWD), &mut streams)
        };
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn exit_status_codes() {
        assert_eq!(ExitStatus::Clean.code(), 0);
        assert_eq!(ExitStatus::Findings.code(), 1);
        assert_eq!(ExitStatus::Error.code(), 2);
    }

    /// Where `init` writes under the injected [`TEST_CWD`].
    const TEST_CONFIG_PATH: &str = "/work/.tzlintrc.json";

    #[test]
    fn init_writes_a_valid_starter_config() {
        let host = MockHost::new();
        let (status, out, _err) = run_capture(&cli(Command::Init { force: false }), &host);
        assert_eq!(status, ExitStatus::Clean);
        // Build the expected path the same way the code does, so the separator matches the
        // platform (`\` on Windows) — the message echoes `path.display()`.
        let expected = Path::new(TEST_CWD).join(".tzlintrc.json");
        assert!(
            out.contains(&format!("created {}", expected.display())),
            "{out}"
        );
        let written = host.read(TEST_CONFIG_PATH).unwrap();
        let config = Config::parse(&written, ConfigFormat::Json).unwrap();
        assert_eq!(config.language.as_deref(), Some("ja"));
    }

    #[test]
    fn init_refuses_to_clobber_without_force() {
        let host = MockHost::with(TEST_CONFIG_PATH, "{ \"language\": \"en\" }");
        let (status, _out, err) = run_capture(&cli(Command::Init { force: false }), &host);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("already exists"), "{err}");
        // The existing file is untouched.
        assert_eq!(
            host.read(TEST_CONFIG_PATH).as_deref(),
            Some("{ \"language\": \"en\" }")
        );
    }

    #[test]
    fn init_force_overwrites() {
        let host = MockHost::with(TEST_CONFIG_PATH, "{ \"language\": \"en\" }");
        let (status, _out, _err) = run_capture(&cli(Command::Init { force: true }), &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(host.read(TEST_CONFIG_PATH).unwrap().contains("\"ja\""));
    }

    #[test]
    fn lint_clean_file_exits_clean() {
        let host = MockHost::with("a.md", "ただのテキストです。\n");
        let (status, out, _err) = run_capture(&lint_cli("a.md", OutputFormat::Text), &host);
        // No rules wired → no diagnostics → Clean.
        assert_eq!(status, ExitStatus::Clean);
        assert!(out.contains("1 file(s) checked, 0 issue(s) found"), "{out}");
    }

    #[test]
    fn lint_missing_file_exits_error() {
        let host = MockHost::new();
        let (status, _out, err) = run_capture(&lint_cli("nope.md", OutputFormat::Text), &host);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("nope.md"), "{err}");
    }

    #[test]
    fn lint_json_output_is_an_array() {
        let host = MockHost::with("a.md", "# 見出し\n\n本文。\n");
        let (status, out, _err) = run_capture(&lint_cli("a.md", OutputFormat::Json), &host);
        assert_eq!(status, ExitStatus::Clean);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(value.is_array());
        assert_eq!(value[0]["path"], "a.md");
        assert_eq!(value[0]["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn lint_sarif_output_is_a_valid_log_with_results() {
        // `--format sarif` emits a SARIF 2.1.0 log; the half-width kana triggers `no-hankaku-kana`,
        // which surfaces as one result referencing that rule.
        let host = MockHost::with("a.md", "ﾊﾛｰ\n");
        let (status, out, _err) = run_capture(&lint_cli("a.md", OutputFormat::Sarif), &host);
        assert_eq!(status, ExitStatus::Findings);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["version"], "2.1.0");
        assert_eq!(value["runs"][0]["tool"]["driver"]["name"], "tzlint");
        let result = &value["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "no-hankaku-kana");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "a.md"
        );
    }

    #[test]
    fn lint_sarif_clean_file_has_no_results() {
        // A clean file still produces a well-formed (empty-results) SARIF log and exits `Clean`.
        let host = MockHost::with("a.md", "ただのテキストです。\n");
        let (status, out, _err) = run_capture(&lint_cli("a.md", OutputFormat::Sarif), &host);
        assert_eq!(status, ExitStatus::Clean);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["runs"][0]["results"], serde_json::json!([]));
    }

    #[test]
    fn no_cache_path_matches_cached_path_with_diagnostics() {
        // Both code paths (cached vs. direct bridge) must produce identical output, including when
        // rules emit diagnostics — half-width kana triggers `no-hankaku-kana` under the default
        // (all-on) rule set.
        let host = MockHost::with("a.md", "ﾊﾛｰ\n");
        let (_s1, cached_out, _e1) = run_capture(&lint_cli("a.md", OutputFormat::Json), &host);
        let mut direct = lint_cli("a.md", OutputFormat::Json);
        direct.no_cache = true;
        let (_s2, direct_out, _e2) = run_capture(&direct, &host);
        assert_eq!(cached_out, direct_out);
        assert!(
            cached_out.contains("no-hankaku-kana"),
            "expected a diagnostic in the output: {cached_out}"
        );
    }

    #[test]
    fn explicit_config_is_loaded_and_noted_when_verbose() {
        let host = MockHost::new();
        host.put("custom.json", "{ \"language\": \"ja\" }");
        host.put("a.md", "x\n");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("custom.json"));
        c.verbose = true;
        let (status, _out, err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(err.contains("using config custom.json"), "{err}");
    }

    #[test]
    fn bad_explicit_config_exits_error() {
        let host = MockHost::with("bad.json", "{ not json");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("bad.json"));
        let (status, _out, err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("bad.json"), "{err}");
    }

    #[test]
    fn fix_makes_no_change_when_there_are_no_fixes() {
        // The built-in rules run, but none produces an autofix for this input, so the file is
        // left untouched and the write branch (`fixed != source`) is not taken.
        let host = MockHost::with("a.md", "本文。\n");
        let (status, out, _err) = run_capture(
            &cli(Command::Fix {
                paths: vec![PathBuf::from("a.md")],
                dry_run: false,
            }),
            &host,
        );
        assert_eq!(status, ExitStatus::Clean);
        assert_eq!(host.read("a.md").as_deref(), Some("本文。\n"));
        assert!(out.contains("0 file(s) changed"), "{out}");
    }

    #[test]
    fn lint_reports_a_violation_and_exits_findings() {
        // Half-width kana triggers `no-hankaku-kana` under the default (all-on) rule set, so the
        // command path reaches `Findings` and renders a real diagnostic.
        let host = MockHost::with("a.md", "ﾊﾛｰ\n");
        let (status, out, _err) = run_capture(&lint_cli("a.md", OutputFormat::Text), &host);
        assert_eq!(status, ExitStatus::Findings);
        assert!(out.contains("no-hankaku-kana"), "{out}");
        assert!(out.contains("1 file(s) checked,"), "{out}");
    }

    #[test]
    fn config_off_disables_a_rule_end_to_end() {
        // Turning the rule off in config makes the same input clean — exercises the full
        // config -> resolve_rules -> engine path.
        let host = MockHost::new();
        host.put("a.md", "ﾊﾛｰ\n");
        host.put("off.json", "{ \"rules\": { \"no-hankaku-kana\": false } }");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("off.json"));
        let (status, out, _err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(out.contains("0 issue(s) found"), "{out}");
    }

    #[test]
    fn config_options_take_effect_end_to_end() {
        // A config that sets `max-ten` to `max: 0` flags a single `、`, proving per-rule options
        // flow config -> resolve_rules -> build_rule -> engine.
        let host = MockHost::new();
        host.put("a.md", "これは、テストです。\n");
        host.put(
            "strict.json",
            "{ \"rules\": { \"max-ten\": { \"options\": { \"max\": 0 } } } }",
        );
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("strict.json"));
        let (status, out, _err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Findings);
        assert!(out.contains("max-ten"), "{out}");
    }

    #[test]
    fn extends_known_preset_loads_and_unknown_errors() {
        // A config that extends a known preset loads cleanly; an unknown preset id is a fatal
        // config error surfaced by the CLI.
        let host = MockHost::new();
        host.put("a.md", "ふつうの文。\n");
        host.put("good.json", "{ \"extends\": \"ja-technical-writing\" }");
        host.put("bad.json", "{ \"extends\": \"no-such-preset\" }");

        let mut good = lint_cli("a.md", OutputFormat::Text);
        good.config = Some(PathBuf::from("good.json"));
        assert_eq!(run_capture(&good, &host).0, ExitStatus::Clean);

        let mut bad = lint_cli("a.md", OutputFormat::Text);
        bad.config = Some(PathBuf::from("bad.json"));
        let (status, _out, err) = run_capture(&bad, &host);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("unknown preset"), "{err}");
    }

    #[test]
    fn unknown_config_rule_id_is_noted() {
        // A misspelled rule id in config is reported (not silently ignored).
        let host = MockHost::new();
        host.put("a.md", "x\n");
        host.put("c.json", "{ \"rules\": { \"no-such-rule\": false } }");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("c.json"));
        let (_status, _out, err) = run_capture(&c, &host);
        assert!(err.contains("unknown rule 'no-such-rule'"), "{err}");
    }

    #[test]
    fn unknown_config_rule_id_is_noted_in_fix() {
        // The same unknown-rule-id warning applies to `fix`, not just `lint`.
        let host = MockHost::new();
        host.put("a.md", "本文。\n");
        host.put("c.json", "{ \"rules\": { \"no-such-rule\": false } }");
        let mut c = cli(Command::Fix {
            paths: vec![PathBuf::from("a.md")],
            dry_run: false,
        });
        c.config = Some(PathBuf::from("c.json"));
        let (status, _out, err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(err.contains("unknown rule 'no-such-rule'"), "{err}");
    }

    #[test]
    fn note_when_every_rule_is_disabled() {
        // Disabling all built-in rules yields an empty rule set, so even an input that would
        // otherwise trigger a rule (half-width kana) is clean — and the "all disabled" note fires.
        use tzlint_rules::RULE_IDS;
        let host = MockHost::new();
        host.put("a.md", "ﾊﾛｰ\n");
        let entries: Vec<String> = RULE_IDS
            .iter()
            .map(|id| format!("\"{id}\": false"))
            .collect();
        host.put(
            "off.json",
            &format!("{{ \"rules\": {{ {} }} }}", entries.join(", ")),
        );
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("off.json"));
        let (status, out, err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(
            err.contains("every built-in rule is disabled by config"),
            "{err}"
        );
        assert!(out.contains("0 issue(s) found"), "{out}");
    }

    #[test]
    fn lint_exit_status_precedence() {
        // Error > Findings > Clean. (The pure-function form covers every combination; the
        // command path reaching `Findings` is covered by `lint_reports_a_violation_*`.)
        assert_eq!(lint_exit_status(false, false), ExitStatus::Clean);
        assert_eq!(lint_exit_status(false, true), ExitStatus::Findings);
        assert_eq!(lint_exit_status(true, false), ExitStatus::Error);
        assert_eq!(lint_exit_status(true, true), ExitStatus::Error);
    }

    #[test]
    fn unrecognized_explicit_config_format_errors() {
        // An explicit --config with an unknown extension fails loudly rather than being
        // mis-parsed as JSONC.
        let host = MockHost::with("config.toml", "x = 1\n");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.config = Some(PathBuf::from("config.toml"));
        let (status, _out, err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("unrecognized config format"), "{err}");
    }

    #[test]
    fn verbose_notes_discovered_config() {
        // With no explicit --config, discovery walks up from the injected cwd and finds the
        // registered file; verbose mode reports it.
        let host = MockHost::new();
        host.put(TEST_CONFIG_PATH, "{ \"language\": \"ja\" }");
        host.put("a.md", "x\n");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.verbose = true;
        let (status, _out, err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        // The discovered path echoes `path.display()`, so build the expectation with the same
        // join to match the platform separator (`\` on Windows).
        let expected = Path::new(TEST_CWD).join(".tzlintrc.json");
        assert!(
            err.contains(&format!("using config {}", expected.display())),
            "{err}"
        );
    }

    #[test]
    fn fix_reports_error_for_unreadable_file_among_others() {
        // One readable (no-op) file and one missing file: the read error makes the run `Error`,
        // and the missing path is named on stderr; nothing is changed (no rules).
        let host = MockHost::with("ok.md", "本文。\n");
        let (status, out, err) = run_capture(
            &cli(Command::Fix {
                paths: vec![PathBuf::from("ok.md"), PathBuf::from("missing.md")],
                dry_run: false,
            }),
            &host,
        );
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("missing.md"), "{err}");
        assert!(out.contains("0 file(s) changed"), "{out}");
    }

    #[test]
    fn lint_writes_cache_file_and_no_cache_skips_it() {
        // A normal lint writes the persistent cache under the (injected) cwd.
        let host = MockHost::with("a.md", "ﾊﾛｰ\n");
        run_capture(&lint_cli("a.md", OutputFormat::Text), &host);
        assert!(
            host.read("/work/.tzlintcache").is_some(),
            "expected a cache file to be written"
        );

        // `--no-cache` neither reads nor writes it.
        let host2 = MockHost::with("a.md", "ﾊﾛｰ\n");
        let mut c = lint_cli("a.md", OutputFormat::Text);
        c.no_cache = true;
        run_capture(&c, &host2);
        assert!(
            host2.read("/work/.tzlintcache").is_none(),
            "--no-cache should not write a cache file"
        );
    }

    #[test]
    fn cache_persists_across_runs() {
        // Run twice against the same host: the second run loads the cache file the first wrote and
        // hits, producing identical output to the fresh lint.
        let host = MockHost::with("a.md", "ﾊﾛｰ\n");
        let (status1, out1, _e1) = run_capture(&lint_cli("a.md", OutputFormat::Json), &host);
        assert!(host.read("/work/.tzlintcache").is_some());
        let (status2, out2, _e2) = run_capture(&lint_cli("a.md", OutputFormat::Json), &host);
        assert_eq!(status1, status2);
        assert_eq!(out1, out2);
        assert!(out2.contains("no-hankaku-kana"), "{out2}");
    }

    #[test]
    fn cache_save_failure_warns_without_failing_the_run() {
        // A host that serves reads but fails every write: the cache save fails, which must only
        // warn (to stderr) — the run still reports its findings, not an error.
        struct ReadOkWriteFailHost {
            source: String,
        }
        impl Host for ReadOkWriteFailHost {
            fn read_to_string(&self, path: &Path, _limit: usize) -> Result<String, IoError> {
                if path.ends_with("a.md") {
                    Ok(self.source.clone())
                } else {
                    Err(IoError::NotFound) // no existing cache file
                }
            }
            fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<(), IoError> {
                Err(IoError::Other("disk full".into()))
            }
            fn exists(&self, _: &Path) -> bool {
                false
            }
        }
        let host = ReadOkWriteFailHost {
            source: "ﾊﾛｰ\n".to_string(),
        };
        let (status, _out, err) = run_capture(&lint_cli("a.md", OutputFormat::Text), &host);
        assert_eq!(status, ExitStatus::Findings); // cache write failure did NOT become an error
        assert!(err.contains("could not write"), "{err}");
    }

    // --- input expansion: globs, directories, stdin (M1g follow-up) ---

    #[test]
    fn lint_expands_a_glob_to_matching_files() {
        // `*.md` anchors at the (injected) cwd and matches only top-level Markdown; the `.txt` is
        // excluded, and two clean files are checked.
        let host = MockHost::new();
        host.put("/work/a.md", "本文。\n");
        host.put("/work/b.md", "別の文。\n");
        host.put("/work/note.txt", "ignored\n");
        let (status, out, _err) = run_capture(&lint_cli("*.md", OutputFormat::Text), &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(out.contains("2 file(s) checked"), "{out}");
    }

    #[test]
    fn lint_directory_arg_lints_markdown_recursively() {
        // A directory argument recurses for `.md`/`.markdown`; other extensions are skipped.
        let host = MockHost::new();
        host.put("docs/a.md", "本文。\n");
        host.put("docs/sub/b.markdown", "別。\n");
        host.put("docs/c.txt", "ignored\n");
        let (status, out, _err) = run_capture(&lint_cli("docs", OutputFormat::Text), &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(out.contains("2 file(s) checked"), "{out}");
    }

    #[test]
    fn lint_invalid_glob_warns_and_matches_nothing() {
        // A malformed glob is reported (not silently ignored) and contributes no files.
        let host = MockHost::new();
        let (status, out, err) = run_capture(&lint_cli("[bad", OutputFormat::Text), &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(err.contains("invalid glob pattern"), "{err}");
        assert!(out.contains("0 file(s) checked"), "{out}");
    }

    #[test]
    fn lint_reads_stdin_under_the_stdin_label() {
        // `-` reads stdin; the diagnostic is labeled `<stdin>` and counted like a file.
        let host = MockHost::new();
        let (status, out, _err) = run_capture_stdin(
            &lint_cli("-", OutputFormat::Text),
            &host,
            "ﾊﾛｰ\n".as_bytes(),
        );
        assert_eq!(status, ExitStatus::Findings);
        assert!(out.contains("<stdin>:"), "{out}");
        assert!(out.contains("no-hankaku-kana"), "{out}");
        assert!(out.contains("1 file(s) checked"), "{out}");
    }

    #[test]
    fn lint_lints_stdin_and_a_file_together() {
        // `-` may be combined with file paths; stdin is reported first, then the file.
        let host = MockHost::with("a.md", "本文。\n");
        let c = cli(Command::Lint {
            paths: vec![PathBuf::from("-"), PathBuf::from("a.md")],
            format: OutputFormat::Text,
        });
        let (status, out, _err) = run_capture_stdin(&c, &host, "ﾊﾛｰ\n".as_bytes());
        assert_eq!(status, ExitStatus::Findings);
        assert!(out.contains("<stdin>"), "{out}");
        assert!(out.contains("2 file(s) checked"), "{out}");
    }

    #[test]
    fn stdin_and_file_yield_identical_diagnostics() {
        // Dispatch parity: the same content linted from stdin and from a file produces identical
        // diagnostics (only the `path` label differs).
        let content = "ﾊﾛｰ\n";
        let stdin_host = MockHost::new();
        let (_s1, stdin_out, _e1) = run_capture_stdin(
            &lint_cli("-", OutputFormat::Json),
            &stdin_host,
            content.as_bytes(),
        );
        let file_host = MockHost::with("x.md", content);
        let (_s2, file_out, _e2) = run_capture(&lint_cli("x.md", OutputFormat::Json), &file_host);

        let stdin_json: serde_json::Value = serde_json::from_str(&stdin_out).unwrap();
        let file_json: serde_json::Value = serde_json::from_str(&file_out).unwrap();
        assert_eq!(stdin_json[0]["diagnostics"], file_json[0]["diagnostics"]);
        assert_eq!(stdin_json[0]["path"], "<stdin>");
        assert_eq!(file_json[0]["path"], "x.md");
    }

    #[test]
    fn fix_stdin_passes_the_document_through_stdout() {
        // No built-in rule autofixes, so the document is unchanged — but `fix -` still echoes it
        // to stdout (a safe pass-through filter); the progress/summary moves to stderr so stdout
        // stays pure data, and nothing is written to the host.
        let host = MockHost::new();
        let c = cli(Command::Fix {
            paths: vec![PathBuf::from("-")],
            dry_run: false,
        });
        let (status, out, err) = run_capture_stdin(&c, &host, "本文。\n".as_bytes());
        assert_eq!(status, ExitStatus::Clean);
        assert_eq!(
            out, "本文。\n",
            "stdout must be exactly the document, no summary"
        );
        assert!(err.contains("<stdin>"), "{err}");
        assert!(host.read("-").is_none(), "stdin must never be written back");
    }

    #[test]
    fn fix_dry_run_stdin_emits_no_document() {
        // `--dry-run` produces no changed artifact, so stdout stays empty for a stdin target.
        let host = MockHost::new();
        let c = cli(Command::Fix {
            paths: vec![PathBuf::from("-")],
            dry_run: true,
        });
        let (status, out, _err) = run_capture_stdin(&c, &host, "本文。\n".as_bytes());
        assert_eq!(status, ExitStatus::Clean);
        assert!(
            out.is_empty(),
            "dry-run stdin must not emit the document: {out:?}"
        );
    }

    #[test]
    fn emit_progress_routes_between_stdout_and_stderr() {
        // The data/metadata split that keeps `fix -`'s stdout pure.
        let (mut out, mut err) = (Vec::new(), Vec::new());
        emit_progress(false, &mut out, &mut err, "to-stdout").unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "to-stdout\n");
        assert!(err.is_empty());

        let (mut out, mut err) = (Vec::new(), Vec::new());
        emit_progress(true, &mut out, &mut err, "to-stderr").unwrap();
        assert!(out.is_empty());
        assert_eq!(String::from_utf8(err).unwrap(), "to-stderr\n");
    }

    #[test]
    fn read_stdin_reads_and_validates_utf8() {
        assert_eq!(
            read_stdin(&mut std::io::Cursor::new(b"hi".to_vec())).unwrap(),
            "hi"
        );
        let err = read_stdin(&mut std::io::Cursor::new(vec![0xff, 0xfe])).unwrap_err();
        assert!(err.contains("invalid UTF-8"), "{err}");
    }

    #[test]
    fn lint_stdin_read_error_exits_error() {
        // Non-UTF-8 on stdin fails the read; `lint -` reports it under `<stdin>` and exits `Error`
        // (rather than panicking or silently skipping the input).
        let host = MockHost::new();
        let (status, _out, err) =
            run_capture_stdin(&lint_cli("-", OutputFormat::Text), &host, &[0xff, 0xfe]);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("error: <stdin>"), "{err}");
        assert!(err.contains("invalid UTF-8"), "{err}");
    }

    // --- `rules` subcommand (M1g follow-up) ---

    #[test]
    fn rules_list_text_defaults_to_all_enabled() {
        use tzlint_rules::RULE_IDS;
        let host = MockHost::new();
        let (status, out, _err) = run_capture(&rules_list_cli(RuleListFormat::Text), &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(
            out.contains(&format!(
                "{} built-in rule(s), {} enabled",
                RULE_IDS.len(),
                RULE_IDS.len()
            )),
            "{out}"
        );
        assert!(
            out.lines()
                .any(|l| l.contains("no-hankaku-kana") && l.contains("on")),
            "{out}"
        );
    }

    #[test]
    fn rules_list_reflects_a_config_disable() {
        // `rules list` honors the resolved config: a rule turned off shows as disabled.
        let host = MockHost::new();
        host.put("off.json", "{ \"rules\": { \"no-hankaku-kana\": false } }");
        let mut c = rules_list_cli(RuleListFormat::Text);
        c.config = Some(PathBuf::from("off.json"));
        let (status, out, _err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(
            out.lines()
                .any(|l| l.contains("no-hankaku-kana") && l.contains("off")),
            "{out}"
        );
    }

    #[test]
    fn rules_list_json_is_an_array_of_rule_objects() {
        let host = MockHost::new();
        let (status, out, _err) = run_capture(&rules_list_cli(RuleListFormat::Json), &host);
        assert_eq!(status, ExitStatus::Clean);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(value.is_array());
        assert_eq!(value[0]["id"], "sentence-length");
        assert_eq!(value[0]["enabled"], true);
    }

    #[test]
    fn rules_explain_known_rule_shows_details() {
        let host = MockHost::new();
        let (status, out, _err) = run_capture(&rules_explain_cli("max-ten"), &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(out.contains("rule:     max-ten"), "{out}");
        assert!(out.contains("status:   enabled"), "{out}");
        assert!(out.contains("severity:"), "{out}");
    }

    #[test]
    fn rules_explain_unknown_rule_exits_error() {
        let host = MockHost::new();
        let (status, _out, err) = run_capture(&rules_explain_cli("no-such-rule"), &host);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("unknown rule 'no-such-rule'"), "{err}");
    }

    #[test]
    fn rules_explain_reflects_config_options() {
        // `explain` surfaces the config-supplied options for a rule.
        let host = MockHost::new();
        host.put(
            "strict.json",
            "{ \"rules\": { \"max-ten\": { \"options\": { \"max\": 0 } } } }",
        );
        let mut c = rules_explain_cli("max-ten");
        c.config = Some(PathBuf::from("strict.json"));
        let (status, out, _err) = run_capture(&c, &host);
        assert_eq!(status, ExitStatus::Clean);
        assert!(out.contains("\"max\":0"), "{out}");
        assert!(out.contains("from config"), "{out}");
    }

    #[test]
    fn fix_stdin_read_error_exits_error() {
        // The same failed-read handling for `fix -`: error on stderr, `Error` exit, no document on
        // stdout.
        let host = MockHost::new();
        let c = cli(Command::Fix {
            paths: vec![PathBuf::from("-")],
            dry_run: false,
        });
        let (status, out, err) = run_capture_stdin(&c, &host, &[0xff, 0xfe]);
        assert_eq!(status, ExitStatus::Error);
        assert!(err.contains("error: <stdin>"), "{err}");
        assert!(out.is_empty(), "no document should be emitted: {out:?}");
    }
}
