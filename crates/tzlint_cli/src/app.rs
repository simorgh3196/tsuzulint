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

use std::io::Write;
use std::path::{Path, PathBuf};

use tzlint_ast::{access, to_archive};
use tzlint_core::io::{MAX_CONFIG, MAX_FILE};
use tzlint_core::{
    Config, ConfigFormat, DocumentCache, Engine, Host, discover, lint_cached, parse,
};
use tzlint_pdk::{Diagnostic, Rule};

use crate::cli::{Cli, Command, OutputFormat};
use crate::output::{self, FileReport};
use crate::rules::resolve_rules;

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
pub fn run(
    cli: &Cli,
    host: &dyn Host,
    cwd: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitStatus {
    let result = match &cli.command {
        Command::Lint { paths, format } => lint(cli, host, cwd, paths, *format, stdout, stderr),
        Command::Fix { paths, dry_run } => fix(cli, host, cwd, paths, *dry_run, stdout, stderr),
        Command::Init { force } => init(host, cwd, *force, stdout, stderr),
    };
    match result {
        Ok(status) => status,
        Err(message) => {
            // stderr is best-effort (see the module's output policy); the `Error` status below
            // is the authoritative signal regardless of whether this message lands.
            let _ = writeln!(stderr, "error: {message}");
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
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<ExitStatus, String> {
    let config = load_config(cli, host, cwd, stderr)?;
    let rules = resolve_rules(&config);
    note_if_no_rules(&rules, stderr);
    let rule_refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();

    let mut cache = (!cli.no_cache).then(DocumentCache::new);
    let mut reports = Vec::with_capacity(paths.len());
    let mut had_error = false;
    for path in paths {
        match read_and_lint(host, cache.as_mut(), path, &config, &rule_refs) {
            Ok(report) => reports.push(report),
            Err(message) => {
                let _ = writeln!(stderr, "error: {}: {message}", path.display());
                had_error = true;
            }
        }
    }

    match format {
        OutputFormat::Text => output::render_text(stdout, &reports).map_err(|e| e.to_string())?,
        OutputFormat::Json => output::render_json(stdout, &reports).map_err(|e| e.to_string())?,
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
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<ExitStatus, String> {
    let config = load_config(cli, host, cwd, stderr)?;
    let rules = resolve_rules(&config);
    note_if_no_rules(&rules, stderr);
    let rule_refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();

    let mut changed = 0usize;
    let mut had_error = false;
    for path in paths {
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
            writeln!(stdout, "would fix {}", path.display()).map_err(|e| e.to_string())?;
        } else if let Err(e) = host.write_atomic(path, fixed.as_bytes()) {
            let _ = writeln!(stderr, "error: {}: {e}", path.display());
            had_error = true;
            continue;
        } else {
            writeln!(stdout, "fixed {}", path.display()).map_err(|e| e.to_string())?;
        }
        changed += 1;
    }

    let verb = if dry_run {
        "would be changed"
    } else {
        "changed"
    };
    writeln!(stdout, "{changed} file(s) {verb}").map_err(|e| e.to_string())?;
    Ok(if had_error {
        ExitStatus::Error
    } else {
        ExitStatus::Clean
    })
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

/// Print a one-line note to stderr when no rules are wired, so an empty result is not mistaken
/// for "your text is clean". Removed once the rule registry lands.
fn note_if_no_rules(rules: &[Box<dyn Rule>], stderr: &mut dyn Write) {
    if rules.is_empty() {
        let _ = writeln!(
            stderr,
            "note: no rules are enabled yet (the built-in rule registry lands in a later step); \
             the pipeline runs but reports nothing"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    use tzlint_core::IoError;

    use crate::cli::Command;

    /// An in-memory [`Host`]: only registered paths exist; writes are recorded so tests can
    /// assert on them.
    struct MockHost {
        files: RefCell<HashMap<PathBuf, String>>,
    }

    impl MockHost {
        fn new() -> Self {
            MockHost {
                files: RefCell::new(HashMap::new()),
            }
        }
        fn with(path: &str, contents: &str) -> Self {
            let host = MockHost::new();
            host.put(path, contents);
            host
        }
        fn put(&self, path: &str, contents: &str) {
            self.files
                .borrow_mut()
                .insert(PathBuf::from(path), contents.to_string());
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
            Ok(())
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
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

    /// A fixed, absolute working directory for hermetic discovery/init tests (the orchestration
    /// never touches the real process cwd — it is injected).
    const TEST_CWD: &str = "/work";

    fn run_capture(cli: &Cli, host: &dyn Host) -> (ExitStatus, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run(cli, host, Path::new(TEST_CWD), &mut out, &mut err);
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
        assert!(out.contains("created /work/.tzlintrc.json"), "{out}");
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
    fn no_cache_path_matches_cached_path() {
        // Both code paths (cached vs. direct bridge) must produce identical output. Today the
        // rule set is empty, so this compares the clean case; the diagnostic-bearing equivalence
        // is added alongside the rule-registry wiring (when a rule can emit a diagnostic).
        let host = MockHost::with("a.md", "段落です。\n別の段落。\n");
        let (_s1, cached_out, _e1) = run_capture(&lint_cli("a.md", OutputFormat::Json), &host);
        let mut direct = lint_cli("a.md", OutputFormat::Json);
        direct.no_cache = true;
        let (_s2, direct_out, _e2) = run_capture(&direct, &host);
        assert_eq!(cached_out, direct_out);
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
    fn fix_is_a_noop_without_rules() {
        // Without rules `fix` makes no change, so the write branch (`fixed != source`) is not
        // reachable here; it is exercised alongside the rule-registry wiring.
        let host = MockHost::with("a.md", "本文。\n");
        let (status, out, _err) = run_capture(
            &cli(Command::Fix {
                paths: vec![PathBuf::from("a.md")],
                dry_run: false,
            }),
            &host,
        );
        assert_eq!(status, ExitStatus::Clean);
        // Unchanged: no rules → no fixes.
        assert_eq!(host.read("a.md").as_deref(), Some("本文。\n"));
        assert!(out.contains("0 file(s) changed"), "{out}");
    }

    #[test]
    fn lint_exit_status_precedence() {
        // Error > Findings > Clean. (Exercises the `Findings` arm without a rule, which the
        // command path cannot reach until the rule registry is wired.)
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
        assert!(err.contains("using config /work/.tzlintrc.json"), "{err}");
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
}
