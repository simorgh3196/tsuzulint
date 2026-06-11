//! Parsing of [`prh`](https://github.com/prh/prh) dictionary files (`.prh.yml`) into a neutral
//! term model the `ja-prh` rule consumes.
//!
//! This module is the **pure parser** half of prh-dictionary support: it turns the YAML text of a
//! `.prh.yml` into a [`PrhDictionary`] (the dictionary's `version`, its `imports` list, and its
//! `rules`), classifying each `pattern` / `patterns` entry as either a [`PrhPattern::Literal`]
//! substring or a [`PrhPattern::Regex`] (a `/source/flags` JS-style regular expression). It does
//! **no** I/O — reading the file, resolving `imports`, and feeding the terms to the rule are the
//! caller's job (the CLI, which owns the [`Host`](crate::io::Host)).
//!
//! ## prh format coverage (0.1.0)
//!
//! Supported: the top-level `version` / `imports` / `rules`, and per-rule `expected` (which doubles
//! as the replacement template — it may carry `$1`-style capture references for a regex pattern),
//! `pattern` (a single string), and `patterns` (a list). A pattern written as `/source/flags` is a
//! regex (only the JS flags `i` and `m` are honored; the rest are accepted and ignored); any other
//! string is a literal. The prh fields `specs`, `options` (e.g. `wordBoundary`), and
//! `regexpMustEmpty` are parsed leniently and **ignored** for now. A rule with no usable pattern is
//! dropped (it could never match).

use std::fmt;

use serde::Deserialize;

/// A parsed prh dictionary: its schema `version`, the relative paths it `imports`, and its `rules`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrhDictionary {
    /// The declared schema `version` (prh uses `1`), or `None` when absent.
    pub version: Option<u64>,
    /// Relative paths to other prh dictionaries to include (resolved by the caller).
    pub imports: Vec<String>,
    /// The terminology rules, in document order, with empty-pattern rules dropped.
    pub rules: Vec<PrhRule>,
}

/// One prh rule: the `expected` spelling (also the replacement template) and the patterns that
/// should be rewritten to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrhRule {
    /// The preferred spelling. For a [`PrhPattern::Regex`] match this is a replacement *template*
    /// and may contain `$1`-style references to the pattern's capture groups.
    pub expected: String,
    /// The disallowed spellings to rewrite, each a literal or a regular expression.
    pub patterns: Vec<PrhPattern>,
}

/// A pattern that should be rewritten to its rule's `expected` spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrhPattern {
    /// A literal substring (matched verbatim, case-sensitively).
    Literal(String),
    /// A JS-style regular expression written `/source/flags` in the dictionary.
    Regex {
        /// The expression source (between the delimiting slashes).
        source: String,
        /// The `i` flag — case-insensitive matching.
        ignore_case: bool,
        /// The `m` flag — `^`/`$` match at line boundaries.
        multiline: bool,
    },
}

/// A failure parsing a prh dictionary.
#[derive(Debug)]
pub enum PrhError {
    /// The text did not parse as YAML, or used an unsupported YAML feature (anchors/aliases).
    Parse(String),
}

impl fmt::Display for PrhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrhError::Parse(message) => write!(f, "failed to parse prh dictionary: {message}"),
        }
    }
}

impl std::error::Error for PrhError {}

/// The JS regular-expression flags prh patterns may carry after the closing `/`. Only `i` and `m`
/// change matching here; the others are accepted (so a real regex pattern is still recognized) but
/// have no effect. Restricting to this set keeps a literal path like `/usr/bin` from being mistaken
/// for a regex (`bin` is not all-flag characters).
const JS_REGEX_FLAGS: &str = "dgimsuvy";

/// Parse the text of a `.prh.yml` dictionary into a [`PrhDictionary`].
///
/// Lenient about content: unknown prh fields are ignored, and a rule with no usable pattern is
/// dropped. Strict about form: malformed YAML — or YAML anchors/aliases, which enable
/// alias-expansion denial-of-service the byte cap does not bound — is a [`PrhError::Parse`].
pub fn parse_prh(text: &str) -> Result<PrhDictionary, PrhError> {
    // A leading BOM is not valid YAML for every parser; strip one, matching the config loader.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim().is_empty() {
        return Ok(PrhDictionary::default());
    }
    // prh dictionaries are read through the size-bounded `Host`, but YAML anchors/aliases can still
    // expand a small document into gigabytes; reject them up front, as the config loader does.
    crate::config::reject_yaml_anchors(text).map_err(PrhError::Parse)?;

    let raw: RawPrh = yaml_serde::from_str(text).map_err(|e| PrhError::Parse(e.to_string()))?;
    Ok(raw.normalize())
}

/// The raw shape deserialized from a `.prh.yml`. Unknown fields (`specs`, `options`,
/// `regexpMustEmpty`, …) are ignored rather than rejected, so a richer real-world dictionary still
/// loads.
#[derive(Deserialize)]
struct RawPrh {
    version: Option<u64>,
    #[serde(default)]
    imports: Vec<String>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

/// A raw rule. `pattern` accepts either a single string or a list (some dictionaries use either);
/// `patterns` is a list. Both are merged into the normalized rule's pattern set.
#[derive(Deserialize)]
struct RawRule {
    expected: Option<String>,
    pattern: Option<StringOrList>,
    patterns: Option<Vec<String>>,
}

/// A YAML scalar that may be a single string or a list of strings.
#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrList {
    One(String),
    Many(Vec<String>),
}

impl StringOrList {
    fn into_vec(self) -> Vec<String> {
        match self {
            StringOrList::One(s) => vec![s],
            StringOrList::Many(v) => v,
        }
    }
}

impl RawPrh {
    fn normalize(self) -> PrhDictionary {
        let rules = self
            .rules
            .into_iter()
            .filter_map(RawRule::normalize)
            .collect();
        PrhDictionary {
            version: self.version,
            imports: self.imports,
            rules,
        }
    }
}

impl RawRule {
    /// Normalize into a [`PrhRule`], or `None` when it lacks an `expected` or has no usable pattern.
    fn normalize(self) -> Option<PrhRule> {
        let expected = self.expected?;
        let mut raw_patterns = self.pattern.map(StringOrList::into_vec).unwrap_or_default();
        if let Some(more) = self.patterns {
            raw_patterns.extend(more);
        }
        let patterns: Vec<PrhPattern> = raw_patterns
            .into_iter()
            .filter_map(|p| classify_pattern(&p))
            .collect();
        if patterns.is_empty() {
            return None;
        }
        Some(PrhRule { expected, patterns })
    }
}

/// Classify a pattern string as a regex (`/source/flags`) or a literal, or drop it when it is a
/// degenerate empty pattern.
fn classify_pattern(raw: &str) -> Option<PrhPattern> {
    if let Some(regex) = parse_regex_literal(raw) {
        return Some(regex);
    }
    if raw.is_empty() {
        return None;
    }
    Some(PrhPattern::Literal(raw.to_string()))
}

/// Recognize a `/source/flags` JS-style regex literal. Returns `None` when `raw` is not a regex
/// literal (no closing slash, an empty source, or trailing characters that are not all valid JS
/// regex flags), so the caller falls back to a literal pattern.
fn parse_regex_literal(raw: &str) -> Option<PrhPattern> {
    let rest = raw.strip_prefix('/')?;
    let close = rest.rfind('/')?;
    let source = &rest[..close];
    let flags = &rest[close + 1..];
    if source.is_empty() {
        return None;
    }
    if !flags.chars().all(|c| JS_REGEX_FLAGS.contains(c)) {
        return None;
    }
    Some(PrhPattern::Regex {
        source: source.to_string(),
        ignore_case: flags.contains('i'),
        multiline: flags.contains('m'),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> PrhPattern {
        PrhPattern::Literal(s.to_string())
    }

    #[test]
    fn parses_version_imports_and_a_literal_patterns_rule() {
        let doc = parse_prh(
            "version: 1\n\
             imports:\n\
             \x20 - ./common.yml\n\
             rules:\n\
             \x20 - expected: ハードウェア\n\
             \x20   patterns:\n\
             \x20     - ハードウエア\n\
             \x20     - ハードウエアー\n",
        )
        .expect("parses");
        assert_eq!(doc.version, Some(1));
        assert_eq!(doc.imports, vec!["./common.yml".to_string()]);
        assert_eq!(doc.rules.len(), 1);
        assert_eq!(doc.rules[0].expected, "ハードウェア");
        assert_eq!(
            doc.rules[0].patterns,
            vec![lit("ハードウエア"), lit("ハードウエアー")]
        );
    }

    #[test]
    fn accepts_a_single_string_pattern() {
        let doc = parse_prh("rules:\n  - expected: JavaScript\n    pattern: Javascript\n")
            .expect("parses");
        assert_eq!(doc.rules.len(), 1);
        assert_eq!(doc.rules[0].patterns, vec![lit("Javascript")]);
    }

    #[test]
    fn classifies_a_regex_pattern_with_capture_template() {
        // The canonical prh form: a /regex/ pattern with an `expected` replacement template.
        let doc = parse_prh("rules:\n  - expected: （$1）\n    pattern: /\\(([^)]+)\\)/\n")
            .expect("parses");
        assert_eq!(doc.rules[0].expected, "（$1）");
        assert_eq!(
            doc.rules[0].patterns,
            vec![PrhPattern::Regex {
                source: "\\(([^)]+)\\)".to_string(),
                ignore_case: false,
                multiline: false,
            }]
        );
    }

    #[test]
    fn honors_the_i_and_m_regex_flags() {
        let doc = parse_prh("rules:\n  - expected: X\n    pattern: /foo/im\n").expect("parses");
        assert_eq!(
            doc.rules[0].patterns,
            vec![PrhPattern::Regex {
                source: "foo".to_string(),
                ignore_case: true,
                multiline: true,
            }]
        );
    }

    #[test]
    fn a_literal_with_slashes_is_not_a_regex() {
        // `/usr/bin` has trailing `bin`, which are not all valid JS regex flags → it stays literal.
        let doc = parse_prh("rules:\n  - expected: X\n    pattern: /usr/bin\n").expect("parses");
        assert_eq!(doc.rules[0].patterns, vec![lit("/usr/bin")]);
    }

    #[test]
    fn merges_pattern_and_patterns() {
        let doc = parse_prh(
            "rules:\n  - expected: E\n    pattern: a\n    patterns:\n      - b\n      - c\n",
        )
        .expect("parses");
        assert_eq!(doc.rules[0].patterns, vec![lit("a"), lit("b"), lit("c")]);
    }

    #[test]
    fn ignores_unknown_prh_fields() {
        // `specs`, `options`, and `regexpMustEmpty` are parsed leniently and ignored.
        let doc = parse_prh(
            "rules:\n  - expected: jQuery\n    pattern: jquery\n    \
             regexpMustEmpty: $1\n    options:\n      wordBoundary: true\n    \
             specs:\n      - from: jquery\n        to: jQuery\n",
        )
        .expect("parses");
        assert_eq!(doc.rules.len(), 1);
        assert_eq!(doc.rules[0].expected, "jQuery");
        assert_eq!(doc.rules[0].patterns, vec![lit("jquery")]);
    }

    #[test]
    fn drops_a_rule_with_no_pattern() {
        // `- expected: Cookie` alone has nothing to match (prh would derive a pattern; we do not),
        // so it is dropped rather than kept as a no-op.
        let doc = parse_prh("rules:\n  - expected: Cookie\n").expect("parses");
        assert!(doc.rules.is_empty());
    }

    #[test]
    fn empty_or_whitespace_is_an_empty_dictionary() {
        assert_eq!(parse_prh("").expect("parses"), PrhDictionary::default());
        assert_eq!(
            parse_prh("  \n\t").expect("parses"),
            PrhDictionary::default()
        );
    }

    #[test]
    fn version_may_be_absent() {
        let doc = parse_prh("rules:\n  - expected: E\n    pattern: p\n").expect("parses");
        assert_eq!(doc.version, None);
    }

    #[test]
    fn malformed_yaml_is_an_error() {
        assert!(matches!(
            parse_prh("rules: [unterminated\n"),
            Err(PrhError::Parse(_))
        ));
    }

    #[test]
    fn yaml_anchors_are_rejected() {
        // Alias-expansion DoS defense, matching the config loader.
        let bomb = "rules: &a\n  - expected: E\n    pattern: p\nmore: *a\n";
        assert!(matches!(parse_prh(bomb), Err(PrhError::Parse(_))));
    }

    #[test]
    fn an_empty_regex_source_falls_back_to_literal() {
        // `//` has an empty source → not a regex; kept as the literal `//`.
        let doc = parse_prh("rules:\n  - expected: E\n    pattern: //\n").expect("parses");
        assert_eq!(doc.rules[0].patterns, vec![lit("//")]);
    }
}
