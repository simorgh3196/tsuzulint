//! Expand the `lint` / `fix` PATH arguments into a concrete, deterministic work list.
//!
//! Each argument is one of:
//! - the stdin sentinel `-` (read once from the injected reader),
//! - a glob pattern (contains `*`, `?`, or `[`) — matched with `glob::Pattern` against paths
//!   discovered by walking [`Host::list_dir`] (never `glob::glob()`, which would touch the
//!   filesystem directly and bypass the io boundary),
//! - a directory — recursively yields its Markdown files,
//! - or a literal file path — passed through verbatim (so an existing file behaves exactly as
//!   before this expansion existed, and a missing one still errors at read time).
//!
//! Discovered files are collected into a [`std::collections::BTreeSet`], so the result is sorted
//! and de-duplicated. Symlinks are never followed (a loop / DoS guard); hidden entries (a leading
//! `.`) are skipped during discovery, which also keeps `.tzlintcache` / `.tzlintrc*` out.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tzlint_core::{EntryKind, Host};

/// The stdin sentinel argument.
const STDIN_ARG: &str = "-";
/// Hard cap on directory-recursion depth below a search root — cheap insurance against
/// pathological nesting even though symlinks (the usual cycle source) are never followed.
const MAX_DEPTH: usize = 64;
/// Soft cap on discovered files; on reaching it, discovery stops and a note is emitted, so a
/// run over an unexpectedly huge tree fails visibly rather than exhausting memory.
const MAX_FILES: usize = 100_000;
/// Extensions a directory / `**`-recursion picks up, matched case-insensitively.
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// The expanded work list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Concrete files to lint/fix, lexicographically sorted and de-duplicated.
    pub files: Vec<PathBuf>,
    /// Whether stdin (`-`) was among the arguments.
    pub stdin: bool,
    /// Best-effort notes to surface on stderr (unreadable subdirectory, depth / file caps).
    pub notes: Vec<String>,
}

/// Expand `args` (the user's PATH arguments) relative to `cwd`.
///
/// `-` requests stdin (collapsed to a single read). An argument with a glob metacharacter
/// (`*`, `?`, `[`) is glob-expanded; a listable directory recurses for Markdown files; anything
/// else is a literal path passed through verbatim (so a missing file still errors at read time).
pub fn expand(host: &dyn Host, cwd: &Path, args: &[PathBuf]) -> Inputs {
    let mut walk = Walk {
        host,
        files: BTreeSet::new(),
        notes: Vec::new(),
        file_cap_hit: false,
        depth_cap_noted: false,
    };
    let mut stdin = false;
    for arg in args {
        if arg.as_os_str() == STDIN_ARG {
            stdin = true;
            continue;
        }
        let as_str = arg.to_string_lossy();
        if is_glob(&as_str) {
            walk.glob(cwd, &as_str);
        } else if host.list_dir(arg).is_ok() {
            walk.directory(arg);
        } else {
            // Not a glob and not a listable directory → a literal file. Pass it through verbatim
            // (no `cwd.join`, no canonicalization): the read reproduces today's behavior exactly,
            // surfacing `NotFound` for a genuinely missing path.
            walk.files.insert(arg.clone());
        }
    }
    Inputs {
        files: walk.files.into_iter().collect(),
        stdin,
        notes: walk.notes,
    }
}

/// Whether `s` contains a glob metacharacter.
fn is_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// Whether `name` (a single path component) is hidden (a leading dot). Hidden files and
/// directories are skipped during discovery, which also keeps `.tzlintcache` / `.tzlintrc*` /
/// `.git` out without special-casing.
fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Whether `path`'s extension is a Markdown extension (case-insensitive).
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            MARKDOWN_EXTENSIONS
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

/// Split a glob pattern into its longest literal directory prefix (used as the walk root, so
/// `src/**/*.md` does not walk the whole tree) and the remainder after it.
///
/// Splitting goes through [`Path::components`] rather than `str::split('/')` so it preserves an
/// absolute root (`/abs/*.md` → base `/abs`, not the relative `abs`) and recognizes the platform
/// separator (`\` on Windows). The remainder is rejoined with `/` — the canonical glob separator —
/// so the `**` / depth detection in [`Walk::glob`] is separator-agnostic.
fn split_literal_prefix(pattern: &str) -> (PathBuf, String) {
    use std::path::Component;
    let mut base = PathBuf::new();
    let mut remainder: Vec<String> = Vec::new();
    let mut in_glob = false;
    for component in Path::new(pattern).components() {
        let part = component.as_os_str().to_string_lossy();
        // A root / prefix / `.` / `..` is structural and never a glob; once the glob part has
        // begun, everything (including a later literal component) belongs to the remainder.
        let is_literal =
            !in_glob && (!matches!(component, Component::Normal(_)) || !is_glob(&part));
        if is_literal {
            base.push(component.as_os_str());
        } else {
            in_glob = true;
            remainder.push(part.into_owned());
        }
    }
    (base, remainder.join("/"))
}

/// What a [`Walk`] selects at each file.
enum Selector<'a> {
    /// Directory recursion: keep Markdown files.
    Markdown,
    /// Glob: keep files whose path matches the pattern.
    Glob(&'a glob::Pattern),
}

/// Accumulates discovered files while walking the host's directory tree.
struct Walk<'a> {
    host: &'a dyn Host,
    files: BTreeSet<PathBuf>,
    notes: Vec<String>,
    /// Set once the file cap is hit; stops all further discovery.
    file_cap_hit: bool,
    /// Set once a depth-cap note has been emitted, to avoid repeating it per branch.
    depth_cap_noted: bool,
}

impl Walk<'_> {
    /// Recurse `dir` for Markdown files.
    fn directory(&mut self, dir: &Path) {
        self.descend(dir, dir, 0, MAX_DEPTH, &Selector::Markdown);
    }

    /// Expand a glob pattern by walking its literal prefix and matching the rest.
    fn glob(&mut self, cwd: &Path, pattern_str: &str) {
        let pattern = match glob::Pattern::new(pattern_str) {
            Ok(pattern) => pattern,
            Err(error) => {
                self.notes
                    .push(format!("invalid glob pattern '{pattern_str}': {error}"));
                return;
            }
        };
        let (base, remainder) = split_literal_prefix(pattern_str);
        // `*` / `?` never cross `/` (only `**` does), so a glob with no `**` matches at a fixed
        // depth below its literal prefix; cap the walk there. With `**`, recurse to the safety cap.
        let max_depth = if remainder.split('/').any(|component| component == "**") {
            MAX_DEPTH
        } else {
            remainder.matches('/').count().min(MAX_DEPTH)
        };
        // A prefix-less pattern (`*.md`, `**/*.md`) anchors its WALK at cwd but matches against
        // cwd-relative paths, so the matched names line up with the pattern the user wrote.
        let (emit_root, match_root) = if base.as_os_str().is_empty() {
            (cwd.to_path_buf(), PathBuf::new())
        } else {
            (base.clone(), base)
        };
        self.descend(
            &emit_root,
            &match_root,
            0,
            max_depth,
            &Selector::Glob(&pattern),
        );
    }

    /// List `emit_dir`, select matching files, and recurse into subdirectories (never symlinks,
    /// never hidden entries) up to `max_depth`. `match_dir` is the path prefix used for glob
    /// matching (equal to `emit_dir` except for prefix-less globs, which match cwd-relative).
    fn descend(
        &mut self,
        emit_dir: &Path,
        match_dir: &Path,
        depth: usize,
        max_depth: usize,
        selector: &Selector,
    ) {
        if self.file_cap_hit {
            return;
        }
        let mut entries = match self.host.list_dir(emit_dir) {
            Ok(entries) => entries,
            Err(error) => {
                // A glob whose literal prefix does not exist simply matches nothing (like
                // find/grep) — silent. An unreadable directory reached mid-walk is a warning.
                if !(depth == 0 && matches!(selector, Selector::Glob(_))) {
                    self.notes
                        .push(format!("could not list {}: {error}", emit_dir.display()));
                }
                return;
            }
        };
        // Sort so recursion order (and thus any cap notes) is deterministic; the file set is a
        // BTreeSet, so the final order does not depend on this, but stable behavior is nicer.
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if self.file_cap_hit {
                return;
            }
            if is_hidden(&entry.name) {
                continue;
            }
            match entry.kind {
                // Never follow symlinks: the primary loop / DoS guard.
                EntryKind::Symlink => continue,
                EntryKind::File => {
                    let emit_path = emit_dir.join(&entry.name);
                    let selected = match selector {
                        Selector::Markdown => is_markdown(&emit_path),
                        Selector::Glob(pattern) => {
                            pattern.matches_path_with(&match_dir.join(&entry.name), match_options())
                        }
                    };
                    if selected {
                        if self.files.len() >= MAX_FILES {
                            self.file_cap_hit = true;
                            self.notes.push(format!(
                                "stopped after {MAX_FILES} files; narrow the inputs with a more specific glob"
                            ));
                            return;
                        }
                        self.files.insert(emit_path);
                    }
                }
                EntryKind::Dir => {
                    if depth + 1 > max_depth {
                        // Only the safety cap is worth reporting; a glob's natural depth limit
                        // (e.g. `*.md` not recursing) is expected and silent.
                        if max_depth == MAX_DEPTH && !self.depth_cap_noted {
                            self.depth_cap_noted = true;
                            self.notes
                                .push(format!("stopped recursing below depth {MAX_DEPTH}"));
                        }
                        continue;
                    }
                    self.descend(
                        &emit_dir.join(&entry.name),
                        &match_dir.join(&entry.name),
                        depth + 1,
                        max_depth,
                        selector,
                    );
                }
            }
        }
    }
}

/// Glob match options: `*`/`?` stay within a path component (only `**` crosses `/`); hidden
/// entries are filtered separately by [`is_hidden`], so leading-dot handling is left default.
fn match_options() -> glob::MatchOptions {
    let mut options = glob::MatchOptions::new();
    options.require_literal_separator = true;
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tzlint_core::{DirEntry, EntryKind, IoError};

    /// An in-memory tree: `files` maps a path to its contents; `dirs` maps a directory to its
    /// immediate children. [`TreeHost::new`] synthesizes the `dirs` map from a list of files so a
    /// test only states the files it cares about.
    struct TreeHost {
        files: HashMap<PathBuf, String>,
        dirs: HashMap<PathBuf, Vec<DirEntry>>,
    }

    impl TreeHost {
        fn new(files: &[&str]) -> Self {
            let mut host = TreeHost {
                files: HashMap::new(),
                dirs: HashMap::new(),
            };
            for path in files {
                host.add_file(path);
            }
            host
        }

        /// Register a file and every (parent → child) directory hop up to (but not including) the
        /// empty root, classifying the leaf as `File` and the intermediates as `Dir`.
        fn add_file(&mut self, path: &str) {
            let leaf = PathBuf::from(path);
            self.files.insert(leaf.clone(), "x".to_string());
            let mut child = leaf.clone();
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
                    let children = self.dirs.entry(parent.to_path_buf()).or_default();
                    if !children.iter().any(|e| e.name == name) {
                        children.push(DirEntry { name, kind });
                    }
                }
                child = parent.to_path_buf();
            }
        }

        /// Add a raw child entry to `dir` (for symlinks and unreadable-subdir fixtures).
        fn add_entry(&mut self, dir: &str, name: &str, kind: EntryKind) {
            self.dirs
                .entry(PathBuf::from(dir))
                .or_default()
                .push(DirEntry {
                    name: name.to_string(),
                    kind,
                });
        }
    }

    impl Host for TreeHost {
        fn read_to_string(&self, path: &Path, _limit: usize) -> Result<String, IoError> {
            self.files.get(path).cloned().ok_or(IoError::NotFound)
        }
        fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<(), IoError> {
            Ok(())
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.contains_key(path) || self.dirs.contains_key(path)
        }
        fn list_dir(&self, dir: &Path) -> Result<Vec<DirEntry>, IoError> {
            self.dirs.get(dir).cloned().ok_or(IoError::NotFound)
        }
    }

    /// `cwd` used when a bare (prefix-less) glob must anchor somewhere.
    const CWD: &str = "/work";

    fn run(host: &TreeHost, args: &[&str]) -> Inputs {
        let args: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
        expand(host, Path::new(CWD), &args)
    }

    fn paths(strs: &[&str]) -> Vec<PathBuf> {
        strs.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn bare_glob_matches_top_level_only_anchored_at_cwd() {
        // A prefix-less `*.md` has no literal directory, so it anchors at cwd and yields cwd-joined
        // paths; `*` does not cross `/`, so nested files are not matched.
        let host = TreeHost::new(&["/work/a.md", "/work/b.md", "/work/c.txt", "/work/sub/d.md"]);
        let out = run(&host, &["*.md"]);
        assert_eq!(out.files, paths(&["/work/a.md", "/work/b.md"]));
        assert!(!out.stdin);
    }

    #[test]
    fn double_star_glob_with_literal_prefix_recurses() {
        // `src/**/*.md` walks from the literal prefix `src` and recurses; the top-level `a.md`
        // outside `src` is not in scope, and `.txt` does not match the pattern.
        let host = TreeHost::new(&["a.md", "src/a.md", "src/lib/b.md", "src/lib/c.txt"]);
        let out = run(&host, &["src/**/*.md"]);
        assert_eq!(out.files, paths(&["src/a.md", "src/lib/b.md"]));
    }

    #[test]
    fn glob_matches_exactly_no_implicit_markdown_filter() {
        // A glob means exactly what it says: `docs/**` matches every file under docs, including
        // `.txt`. (Only DIRECTORY args default to Markdown — see `directory_arg_*`.)
        let host = TreeHost::new(&["docs/a.md", "docs/lib/b.markdown", "docs/c.txt"]);
        let out = run(&host, &["docs/**"]);
        assert_eq!(
            out.files,
            paths(&["docs/a.md", "docs/c.txt", "docs/lib/b.markdown"])
        );
    }

    #[test]
    fn directory_arg_recurses_markdown_only() {
        // A literal directory yields its `.md`/`.markdown` files recursively; other extensions,
        // hidden files, and hidden subdirectories are skipped.
        let mut host = TreeHost::new(&[
            "docs/a.md",
            "docs/lib/b.markdown",
            "docs/c.txt",
            "docs/.hidden.md",
            "docs/.git/x.md",
        ]);
        // `.git` is registered as a hidden dir via add_file already; make sure a normal file there
        // would be excluded too (it is, since `.git` is skipped before recursion).
        let _ = &mut host;
        let out = run(&host, &["docs"]);
        assert_eq!(out.files, paths(&["docs/a.md", "docs/lib/b.markdown"]));
    }

    #[test]
    fn directory_arg_markdown_extension_is_case_insensitive() {
        let host = TreeHost::new(&["docs/README.MD", "docs/x.MARKDOWN", "docs/y.TXT"]);
        let out = run(&host, &["docs"]);
        assert_eq!(out.files, paths(&["docs/README.MD", "docs/x.MARKDOWN"]));
    }

    #[test]
    fn directory_arg_never_yields_cache_or_config_dotfiles() {
        let host = TreeHost::new(&[
            "p/a.md",
            "p/.tzlintcache",
            "p/.tzlintrc.json",
            "p/.gitignore",
        ]);
        let out = run(&host, &["p"]);
        assert_eq!(out.files, paths(&["p/a.md"]));
    }

    #[test]
    fn literal_file_is_passed_through_verbatim() {
        // No glob chars and not a listable directory → the exact PathBuf, so existing single-file
        // behavior (and labeling) is unchanged.
        let host = TreeHost::new(&["a.md"]);
        let out = run(&host, &["a.md"]);
        assert_eq!(out.files, paths(&["a.md"]));
        assert!(out.notes.is_empty());
    }

    #[test]
    fn missing_literal_is_passed_through_for_the_reader_to_error() {
        // expand does not probe file existence; a missing literal flows through and the caller's
        // read surfaces the NotFound error (preserving today's missing-file behavior).
        let host = TreeHost::new(&[]);
        let out = run(&host, &["nope.md"]);
        assert_eq!(out.files, paths(&["nope.md"]));
    }

    #[test]
    fn glob_matching_nothing_yields_no_files() {
        let host = TreeHost::new(&["/work/a.txt"]);
        let out = run(&host, &["*.md"]);
        assert!(out.files.is_empty());
        assert!(out.notes.is_empty());
    }

    #[test]
    fn glob_with_nonexistent_base_is_silent() {
        // A glob whose literal prefix does not exist matches nothing WITHOUT a note (like
        // find/grep) — the unreadable-directory warning is only for subdirectories reached
        // mid-walk, not the top-level base.
        let host = TreeHost::new(&["docs/a.md"]);
        let out = run(&host, &["nope/**/*.md"]);
        assert!(out.files.is_empty(), "{:?}", out.files);
        assert!(
            out.notes.is_empty(),
            "no warning for an absent base: {:?}",
            out.notes
        );
    }

    #[test]
    fn overlapping_dir_and_glob_lint_each_file_once() {
        // `docs` (dir) and `docs/**/*.md` (glob) both yield docs/a.md → deduped by the BTreeSet.
        let host = TreeHost::new(&["docs/a.md", "docs/lib/b.md"]);
        let out = run(&host, &["docs", "docs/**/*.md"]);
        assert_eq!(out.files, paths(&["docs/a.md", "docs/lib/b.md"]));
    }

    #[test]
    fn result_is_deterministic_and_sorted() {
        let host = TreeHost::new(&["docs/z.md", "docs/a.md", "docs/m.md"]);
        let first = run(&host, &["docs"]);
        let second = run(&host, &["docs"]);
        assert_eq!(first.files, second.files);
        assert_eq!(first.files, paths(&["docs/a.md", "docs/m.md", "docs/z.md"]));
    }

    #[test]
    fn directory_symlink_is_not_followed() {
        // `docs` contains a symlink child `link` (to anything). The walker must not descend into
        // it, so a file that would only be reachable through the link is absent.
        let mut host = TreeHost::new(&["docs/a.md", "linktarget/secret.md"]);
        host.add_entry("docs", "link", EntryKind::Symlink);
        // Even if the link "pointed" at linktarget (registered as a real dir), we never recurse.
        let out = run(&host, &["docs"]);
        assert_eq!(out.files, paths(&["docs/a.md"]));
    }

    #[test]
    fn recursion_depth_is_capped_with_a_note() {
        // Build a chain deeper than the cap under `deep/`; discovery stops and notes it, without
        // panicking, and still completes.
        let mut deep = String::from("deep");
        for i in 0..70 {
            deep.push_str(&format!("/d{i}"));
        }
        let leaf = format!("{deep}/x.md");
        let host = TreeHost::new(&[&leaf]);
        let out = run(&host, &["deep"]);
        // The deeply-nested file is beyond the cap, so it is not collected, and a note explains it.
        assert!(out.files.is_empty(), "{:?}", out.files);
        assert!(
            out.notes.iter().any(|n| n.contains("depth")),
            "expected a depth-cap note, got {:?}",
            out.notes
        );
    }

    #[test]
    fn unreadable_nested_subdir_notes_and_continues() {
        // `docs` lists a `secret` subdir that cannot be listed (no dirs entry → Err); its sibling
        // file is still collected and a warning note is emitted.
        let mut host = TreeHost::new(&["docs/a.md"]);
        host.add_entry("docs", "secret", EntryKind::Dir); // but no dirs[docs/secret] registered
        let out = run(&host, &["docs"]);
        assert_eq!(out.files, paths(&["docs/a.md"]));
        assert!(
            out.notes.iter().any(|n| n.contains("secret")),
            "expected an unreadable-subdir note, got {:?}",
            out.notes
        );
    }

    #[test]
    fn unreadable_or_missing_top_level_dir_becomes_a_literal() {
        // A non-glob arg that is not a listable directory is treated as a literal path (the read
        // will error later); expand itself does not fail.
        let host = TreeHost::new(&[]);
        let out = run(&host, &["unreadable"]);
        assert_eq!(out.files, paths(&["unreadable"]));
    }

    #[test]
    fn stdin_sentinel_sets_the_flag_and_is_not_a_file() {
        let host = TreeHost::new(&["a.md"]);
        let out = run(&host, &["-", "a.md"]);
        assert!(out.stdin);
        assert_eq!(out.files, paths(&["a.md"]));
    }

    #[test]
    fn repeated_stdin_sentinels_collapse() {
        let host = TreeHost::new(&[]);
        let out = run(&host, &["-", "-"]);
        assert!(out.stdin);
        assert!(out.files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn absolute_glob_preserves_the_root() {
        // The leading `/` must survive prefix extraction so the walk anchors at the real root and
        // the pattern matches the absolute candidate (regression: a naive `split('/')` +
        // `PathBuf::push("")` dropped the slash, silently matching nothing).
        let host = TreeHost::new(&["/abs/a.md", "/abs/b.txt"]);
        let out = run(&host, &["/abs/*.md"]);
        assert_eq!(out.files, paths(&["/abs/a.md"]));
    }

    #[cfg(unix)]
    #[test]
    fn split_literal_prefix_keeps_an_absolute_base() {
        let (base, remainder) = split_literal_prefix("/abs/*.md");
        assert!(base.is_absolute(), "base should stay absolute: {base:?}");
        assert_eq!(base, PathBuf::from("/abs"));
        assert_eq!(remainder, "*.md");
    }

    #[cfg(windows)]
    #[test]
    fn split_literal_prefix_handles_backslash_separators() {
        // On Windows the platform separator is `\`; the prefix split must recognize it so a
        // backslash glob is not collapsed into a single (non-recursing) component.
        let (base, remainder) = split_literal_prefix(r"src\**\*.md");
        assert_eq!(base, PathBuf::from("src"));
        assert!(
            remainder.split('/').any(|component| component == "**"),
            "remainder should expose the `**` component: {remainder}"
        );
    }
}
