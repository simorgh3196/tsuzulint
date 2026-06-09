//! Loading [`prh`](https://github.com/prh/prh) `.prh.yml` dictionaries configured on the `ja-prh`
//! rule and folding their terms into that rule's options before linting.
//!
//! The rule itself stays pure (no I/O): the CLI owns the [`Host`], so it is the CLI that reads the
//! dictionary files named in `rules.ja-prh.options.dictionaries`, parses them with
//! [`tzlint_core::parse_prh`], follows their `imports`, and appends the resulting terms to the
//! rule's `options.terms` (the same shape an inline term takes). A dictionary that cannot be read
//! or parsed is reported on stderr and skipped — best-effort prh migration, never a hard failure.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value, json};
use tzlint_core::io::MAX_CONFIG;
use tzlint_core::{Config, Host, PrhPattern, PrhRule, RuleSetting, parse_prh};
use tzlint_pdk::RuleId;

/// The rule whose `dictionaries` option this module resolves.
const JA_PRH_ID: &str = "ja-prh";

/// Bound on `imports` recursion so a deep or pathological include chain cannot run away (a cycle is
/// already stopped by the visited set; this also caps a very long linear chain).
const MAX_IMPORT_DEPTH: usize = 16;

/// Resolve the `ja-prh` rule's `dictionaries` option in place.
///
/// Each entry is a path to a `.prh.yml` file (relative paths resolved against `base_dir`, the
/// directory of the config file). Every file is read through `host`, parsed, and its `imports`
/// followed; the accumulated terms are appended to the rule's `options.terms`, after any inline
/// terms. A no-op when `ja-prh` is disabled or names no dictionaries.
pub fn apply_prh_dictionaries(
    config: &mut Config,
    host: &dyn Host,
    base_dir: &Path,
    stderr: &mut dyn Write,
) {
    let id = RuleId::from(JA_PRH_ID);
    let (severity, mut options) = match config.rules.get(&id) {
        Some(RuleSetting::On { severity, options }) => (*severity, options.clone()),
        // Off, or absent (the rule then runs with default — empty — options, so no dictionaries).
        _ => return,
    };

    let dict_paths: Vec<String> = options
        .get("dictionaries")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if dict_paths.is_empty() {
        return;
    }

    let mut visited = BTreeSet::new();
    let mut terms: Vec<Value> = Vec::new();
    for rel in &dict_paths {
        let path = normalize(&base_dir.join(rel));
        load_into(&path, host, &mut visited, 0, stderr, &mut terms);
    }
    if terms.is_empty() {
        return;
    }

    append_terms(&mut options, terms);
    config
        .rules
        .insert(id, RuleSetting::On { severity, options });
}

/// Read, parse, and recursively `import` the dictionary at `path`, appending each rule's term to
/// `terms`. Errors are noted on `stderr` and skipped. The `visited` set (keyed by the normalized
/// path) stops cycles and avoids loading a diamond-included file twice.
fn load_into(
    path: &Path,
    host: &dyn Host,
    visited: &mut BTreeSet<PathBuf>,
    depth: usize,
    stderr: &mut dyn Write,
    terms: &mut Vec<Value>,
) {
    if depth > MAX_IMPORT_DEPTH {
        let _ = writeln!(
            stderr,
            "note: ja-prh: prh import depth limit reached at {}",
            path.display()
        );
        return;
    }
    if !visited.insert(path.to_path_buf()) {
        return; // already loaded (a cycle, or a diamond include)
    }
    let text = match host.read_to_string(path, MAX_CONFIG) {
        Ok(text) => text,
        Err(e) => {
            let _ = writeln!(
                stderr,
                "note: ja-prh: could not read prh dictionary {}: {e}",
                path.display()
            );
            return;
        }
    };
    let dict = match parse_prh(&text) {
        Ok(dict) => dict,
        Err(e) => {
            let _ = writeln!(stderr, "note: ja-prh: {}: {e}", path.display());
            return;
        }
    };
    // Imports are resolved relative to *this* file's directory.
    let parent = path.parent().unwrap_or(Path::new(""));
    for import in &dict.imports {
        load_into(
            &normalize(&parent.join(import)),
            host,
            visited,
            depth + 1,
            stderr,
            terms,
        );
    }
    for rule in &dict.rules {
        terms.push(rule_to_term(rule));
    }
}

/// Convert a parsed [`PrhRule`] into the `ja-prh` term JSON: `{ expected, patterns?, regexPatterns? }`.
fn rule_to_term(rule: &PrhRule) -> Value {
    let mut literals: Vec<Value> = Vec::new();
    let mut regexes: Vec<Value> = Vec::new();
    for pattern in &rule.patterns {
        match pattern {
            PrhPattern::Literal(s) => literals.push(Value::String(s.clone())),
            PrhPattern::Regex {
                source,
                ignore_case,
                multiline,
            } => regexes.push(json!({
                "source": source,
                "ignoreCase": ignore_case,
                "multiline": multiline,
            })),
        }
    }
    let mut term = Map::new();
    term.insert("expected".to_string(), Value::String(rule.expected.clone()));
    if !literals.is_empty() {
        term.insert("patterns".to_string(), Value::Array(literals));
    }
    if !regexes.is_empty() {
        term.insert("regexPatterns".to_string(), Value::Array(regexes));
    }
    Value::Object(term)
}

/// Append `loaded` terms to `options.terms` (creating the array if needed), keeping any inline
/// terms first. `options` is the rule's options object (it is, since we read `dictionaries` from
/// it); a non-object is left untouched.
fn append_terms(options: &mut Value, loaded: Vec<Value>) {
    let Value::Object(obj) = options else { return };
    match obj.get_mut("terms") {
        Some(Value::Array(existing)) => existing.extend(loaded),
        _ => {
            obj.insert("terms".to_string(), Value::Array(loaded));
        }
    }
}

/// Lexically normalize a path — collapse `.` and resolve `..` components — so paths reached by
/// different spellings (`a/./b`, `a/../a/b`) compare equal in the `visited` cycle guard and read
/// the same file.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tzlint_core::{DirEntry, IoError};

    /// A minimal in-memory [`Host`]: only registered paths exist; only reads are exercised here.
    struct MapHost {
        files: BTreeMap<PathBuf, String>,
    }

    impl MapHost {
        fn new(files: &[(&str, &str)]) -> Self {
            MapHost {
                files: files
                    .iter()
                    .map(|(p, c)| (PathBuf::from(p), c.to_string()))
                    .collect(),
            }
        }
    }

    impl Host for MapHost {
        fn read_to_string(&self, path: &Path, _limit: usize) -> Result<String, IoError> {
            self.files.get(path).cloned().ok_or(IoError::NotFound)
        }
        fn write_atomic(&self, _path: &Path, _contents: &[u8]) -> Result<(), IoError> {
            Err(IoError::Other("read-only test host".to_string()))
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }
        fn list_dir(&self, _dir: &Path) -> Result<Vec<DirEntry>, IoError> {
            Ok(Vec::new())
        }
    }

    /// A config with `ja-prh` enabled and the given options.
    fn config_with_options(options: Value) -> Config {
        let mut rules = BTreeMap::new();
        rules.insert(
            RuleId::from(JA_PRH_ID),
            RuleSetting::On {
                severity: None,
                options,
            },
        );
        Config {
            rules,
            ..Default::default()
        }
    }

    /// The `terms` array folded into the ja-prh options after resolution.
    fn resolved_terms(config: &Config) -> Vec<Value> {
        let RuleSetting::On { options, .. } = config.rules.get(&RuleId::from(JA_PRH_ID)).unwrap()
        else {
            panic!("ja-prh missing");
        };
        options
            .get("terms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn run(config: &mut Config, host: &dyn Host) -> String {
        let mut stderr = Vec::new();
        apply_prh_dictionaries(config, host, Path::new("/work"), &mut stderr);
        String::from_utf8(stderr).unwrap()
    }

    #[test]
    fn loads_literal_and_regex_terms_from_a_dictionary() {
        let host = MapHost::new(&[(
            "/work/web.prh.yml",
            "rules:\n  - expected: JavaScript\n    patterns: [Javascript]\n  \
             - expected: （$1）\n    pattern: /\\(([^)]+)\\)/\n",
        )]);
        let mut config = config_with_options(json!({ "dictionaries": ["web.prh.yml"] }));
        assert!(run(&mut config, &host).is_empty());
        let terms = resolved_terms(&config);
        assert_eq!(terms.len(), 2, "{terms:?}");
        assert_eq!(terms[0]["expected"], json!("JavaScript"));
        assert_eq!(terms[0]["patterns"], json!(["Javascript"]));
        assert_eq!(terms[1]["expected"], json!("（$1）"));
        assert_eq!(
            terms[1]["regexPatterns"][0]["source"],
            json!("\\(([^)]+)\\)")
        );
    }

    #[test]
    fn inline_terms_are_kept_before_loaded_ones() {
        let host = MapHost::new(&[(
            "/work/d.prh.yml",
            "rules:\n  - expected: B\n    pattern: b\n",
        )]);
        let mut config = config_with_options(json!({
            "terms": [{ "expected": "A", "patterns": ["a"] }],
            "dictionaries": ["d.prh.yml"],
        }));
        run(&mut config, &host);
        let terms = resolved_terms(&config);
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0]["expected"], json!("A")); // inline first
        assert_eq!(terms[1]["expected"], json!("B")); // loaded after
    }

    #[test]
    fn follows_imports_relative_to_the_importing_file() {
        let host = MapHost::new(&[
            (
                "/work/main.prh.yml",
                "imports:\n  - ./sub/base.prh.yml\nrules:\n  - expected: Main\n    pattern: main\n",
            ),
            (
                "/work/sub/base.prh.yml",
                "rules:\n  - expected: Base\n    pattern: base\n",
            ),
        ]);
        let mut config = config_with_options(json!({ "dictionaries": ["main.prh.yml"] }));
        run(&mut config, &host);
        let expecteds: Vec<String> = resolved_terms(&config)
            .iter()
            .filter_map(|t| t["expected"].as_str().map(str::to_string))
            .collect();
        // Imported terms load before the importing file's own.
        assert_eq!(expecteds, vec!["Base".to_string(), "Main".to_string()]);
    }

    #[test]
    fn a_cyclic_import_terminates() {
        let host = MapHost::new(&[
            (
                "/work/a.prh.yml",
                "imports:\n  - ./b.prh.yml\nrules:\n  - expected: A\n    pattern: a\n",
            ),
            (
                "/work/b.prh.yml",
                "imports:\n  - ./a.prh.yml\nrules:\n  - expected: B\n    pattern: b\n",
            ),
        ]);
        let mut config = config_with_options(json!({ "dictionaries": ["a.prh.yml"] }));
        run(&mut config, &host); // must not loop forever
        // Each file loads exactly once.
        assert_eq!(resolved_terms(&config).len(), 2);
    }

    #[test]
    fn a_missing_dictionary_is_noted_and_skipped() {
        let host = MapHost::new(&[]);
        let mut config = config_with_options(json!({ "dictionaries": ["nope.prh.yml"] }));
        let notes = run(&mut config, &host);
        assert!(notes.contains("could not read"), "{notes}");
        assert!(resolved_terms(&config).is_empty());
    }

    #[test]
    fn no_dictionaries_option_is_a_no_op() {
        let host = MapHost::new(&[]);
        let mut config =
            config_with_options(json!({ "terms": [{ "expected": "A", "patterns": ["a"] }] }));
        run(&mut config, &host);
        // Untouched: just the inline term.
        assert_eq!(resolved_terms(&config).len(), 1);
    }

    #[test]
    fn disabled_ja_prh_is_left_alone() {
        let host = MapHost::new(&[(
            "/work/d.prh.yml",
            "rules:\n  - expected: B\n    pattern: b\n",
        )]);
        let mut config = Config::default();
        config
            .rules
            .insert(RuleId::from(JA_PRH_ID), RuleSetting::Off);
        run(&mut config, &host);
        assert!(matches!(
            config.rules.get(&RuleId::from(JA_PRH_ID)),
            Some(RuleSetting::Off)
        ));
    }

    #[test]
    fn normalize_collapses_dot_and_parent_components() {
        assert_eq!(
            normalize(Path::new("/work/./sub/../a.yml")),
            PathBuf::from("/work/a.yml")
        );
    }
}
