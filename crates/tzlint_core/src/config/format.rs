//! Config file formats: detection by name, the JSONC pre-processor, and parse dispatch.

use std::fmt;
use std::path::Path;

use super::model::RawConfig;
use super::{Config, ConfigError};

/// A supported config file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// Strict JSON (`.json`).
    Json,
    /// JSON with `//`/`/* */` comments and trailing commas (`.jsonc`, and the extensionless
    /// `.tzlintrc`).
    Jsonc,
    /// YAML (`.yaml`, `.yml`).
    Yaml,
}

impl fmt::Display for ConfigFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfigFormat::Json => "JSON",
            ConfigFormat::Jsonc => "JSONC",
            ConfigFormat::Yaml => "YAML",
        })
    }
}

impl ConfigFormat {
    /// Infer the format from a path's file name, or `None` if it is not a recognized config
    /// file. The extension match is ASCII-case-insensitive (so `.JSON` works on
    /// case-insensitive filesystems), and the bare extension dotfiles (`.json`, `.yaml`, …)
    /// are *not* recognized — only the extensionless `.tzlintrc` may have an empty stem, and
    /// it is treated as JSONC.
    pub fn from_path(path: &Path) -> Option<ConfigFormat> {
        let name = path.file_name()?.to_str()?;
        let (stem, ext) = name.rsplit_once('.')?;
        match (stem, ext.to_ascii_lowercase().as_str()) {
            (stem, "json") if !stem.is_empty() => Some(ConfigFormat::Json),
            (stem, "jsonc") if !stem.is_empty() => Some(ConfigFormat::Jsonc),
            (stem, "yaml" | "yml") if !stem.is_empty() => Some(ConfigFormat::Yaml),
            // `.tzlintrc` has no second dot: `rsplit_once('.')` yields `("", "tzlintrc")`.
            ("", "tzlintrc") => Some(ConfigFormat::Jsonc),
            _ => None,
        }
    }
}

/// Parse `text` in `format` into a resolved [`Config`].
pub(super) fn parse(text: &str, format: ConfigFormat) -> Result<Config, ConfigError> {
    let parse_err = |message: String| ConfigError::Parse { format, message };
    // A leading UTF-8 BOM (common from Windows/editor saves) is not valid JSON, while YAML
    // tolerates it; strip one so every format behaves the same.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // An empty or whitespace-only document is the default config, uniformly across formats
    // (serde_json rejects an empty document outright, and some whitespace is not a valid YAML
    // token either). A comments-only JSONC file is handled after stripping, below.
    if text.trim().is_empty() {
        return Ok(Config::default());
    }
    let raw: RawConfig = match format {
        ConfigFormat::Json => serde_json::from_str(text).map_err(|e| parse_err(e.to_string()))?,
        ConfigFormat::Jsonc => {
            let stripped = strip_jsonc(text);
            if stripped.trim().is_empty() {
                return Ok(Config::default()); // e.g. a comments-only file
            }
            serde_json::from_str(&stripped).map_err(|e| parse_err(e.to_string()))?
        }
        ConfigFormat::Yaml => {
            // YAML anchors/aliases enable alias-expansion ("billion laughs") memory-exhaustion
            // that the MAX_CONFIG byte cap does not bound; reject them up front.
            reject_yaml_anchors(text).map_err(parse_err)?;
            yaml_serde::from_str(text).map_err(|e| parse_err(e.to_string()))?
        }
    };
    raw.into_config()
}

/// Strip JSONC extensions (line/block comments and trailing commas) to plain JSON.
///
/// String-aware: `//`, `/* */`, and trailing commas are only treated as such *outside* string
/// literals (escaped quotes inside strings are honored). Trailing commas are removed by, at
/// each `}`/`]`, dropping any immediately preceding whitespace and a single comma — comments
/// were already elided, so only whitespace can sit between the comma and the bracket.
fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                i += 2;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                // Skip the closing `*/`, clamped so an unterminated comment lands at EOF and
                // keeps the `i <= chars.len()` invariant.
                i = (i + 2).min(chars.len());
            }
            '}' | ']' => {
                while out.ends_with(|w: char| w.is_ascii_whitespace()) {
                    out.pop();
                }
                if out.ends_with(',') {
                    out.pop();
                }
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }

    out
}

/// Reject YAML anchors (`&name`) and aliases (`*name`) in config files.
///
/// They are unnecessary for configuration and enable alias-expansion ("billion laughs")
/// memory-exhaustion: a sub-1-MiB document can expand to multiple gigabytes during
/// deserialization, so the [`MAX_CONFIG`](crate::io::MAX_CONFIG) byte cap does not bound it.
/// The scan is string- and comment-aware: a `&`/`*` inside a quoted scalar (`"…"`/`'…'`) or a
/// `#` comment is ignored, and a `&`/`*` is only flagged when it begins a token (preceded by
/// start-of-input, whitespace, or a flow indicator) and is followed by a name character — so
/// arithmetic-like plain scalars such as `a & b` or `2 * 3` are not flagged.
fn reject_yaml_anchors(text: &str) -> Result<(), String> {
    #[derive(PartialEq)]
    enum State {
        Plain,
        Single,
        Double,
        Comment,
    }

    let chars: Vec<char> = text.chars().collect();
    let mut state = State::Plain;
    let mut escaped = false;
    let mut prev: Option<char> = None;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match state {
            State::Double => {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    state = State::Plain;
                }
            }
            State::Single => {
                if c == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
                        i += 1; // `''` is an escaped single quote: stay in the string
                    } else {
                        state = State::Plain;
                    }
                }
            }
            State::Comment => {
                if c == '\n' {
                    state = State::Plain;
                }
            }
            State::Plain => match c {
                '"' => state = State::Double,
                '\'' => state = State::Single,
                '#' if matches!(prev, None | Some(' ' | '\t' | '\n' | '\r')) => {
                    state = State::Comment;
                }
                '&' | '*' if is_node_start(prev) && chars.get(i + 1).is_some_and(is_name_char) => {
                    let kind = if c == '&' {
                        "anchors (&)"
                    } else {
                        "aliases (*)"
                    };
                    return Err(format!(
                        "YAML {kind} are not supported in config files \
                         (they enable alias-expansion denial-of-service); \
                         remove it or quote the value"
                    ));
                }
                _ => {}
            },
        }
        prev = Some(c);
        i += 1;
    }
    Ok(())
}

/// Whether a `&`/`*` at this position could begin a YAML node (so an anchor/alias indicator),
/// based on the preceding character: start-of-input, whitespace, or a flow indicator.
fn is_node_start(prev: Option<char>) -> bool {
    matches!(
        prev,
        None | Some(' ' | '\t' | '\n' | '\r' | '[' | '{' | ',')
    )
}

/// Whether `c` can start a YAML anchor name (conservatively: any non-space char that is not a
/// flow terminator or comment marker).
fn is_name_char(c: &char) -> bool {
    !c.is_whitespace() && !matches!(c, ',' | ']' | '}' | '#')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_detects_each_format() {
        let f = |p: &str| ConfigFormat::from_path(Path::new(p));
        assert_eq!(f("/x/.tzlintrc.jsonc"), Some(ConfigFormat::Jsonc));
        assert_eq!(f("/x/.tzlintrc.json"), Some(ConfigFormat::Json));
        assert_eq!(f("/x/.tzlintrc.yaml"), Some(ConfigFormat::Yaml));
        assert_eq!(f("/x/.tzlintrc.yml"), Some(ConfigFormat::Yaml));
        assert_eq!(f("/x/.tzlintrc"), Some(ConfigFormat::Jsonc));
        assert_eq!(f("/x/config.toml"), None);
        assert_eq!(f("/x/README"), None);
    }

    #[test]
    fn from_path_is_case_insensitive() {
        let f = |p: &str| ConfigFormat::from_path(Path::new(p));
        assert_eq!(f("/x/.tzlintrc.JSON"), Some(ConfigFormat::Json));
        assert_eq!(f("/x/config.YAML"), Some(ConfigFormat::Yaml));
        assert_eq!(f("/x/.tzlintrc.YML"), Some(ConfigFormat::Yaml));
    }

    #[test]
    fn from_path_rejects_bare_extension_dotfiles() {
        let f = |p: &str| ConfigFormat::from_path(Path::new(p));
        assert_eq!(f("/x/.json"), None);
        assert_eq!(f("/x/.yaml"), None);
        assert_eq!(f("/x/.jsonc"), None);
    }

    #[test]
    fn display_names() {
        assert_eq!(ConfigFormat::Json.to_string(), "JSON");
        assert_eq!(ConfigFormat::Jsonc.to_string(), "JSONC");
        assert_eq!(ConfigFormat::Yaml.to_string(), "YAML");
    }

    #[test]
    fn jsonc_strips_line_and_block_comments() {
        let src = r#"{
            // a line comment
            "language": "ja", /* inline block */
            "rules": {} /* trailing */
        }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["language"], serde_json::json!("ja"));
    }

    #[test]
    fn jsonc_strips_trailing_commas() {
        let src = r#"{ "a": [1, 2, 3,], "b": { "c": 1, }, }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["a"], serde_json::json!([1, 2, 3]));
        assert_eq!(v["b"]["c"], serde_json::json!(1));
    }

    #[test]
    fn jsonc_preserves_comment_markers_inside_strings() {
        // `//`, `/*`, and a `}` inside a string must NOT be treated as comments/structure.
        let src = r#"{ "url": "http://example.com/*not a comment*/", "brace": "a}b" }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(
            v["url"],
            serde_json::json!("http://example.com/*not a comment*/")
        );
        assert_eq!(v["brace"], serde_json::json!("a}b"));
    }

    #[test]
    fn jsonc_honors_escaped_quotes_in_strings() {
        let src = r#"{ "q": "a \" // still string", "n": 1, }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["q"], serde_json::json!("a \" // still string"));
        assert_eq!(v["n"], serde_json::json!(1));
    }

    #[test]
    fn jsonc_does_not_strip_comma_inside_string() {
        // A comma inside a string just before a `}` must survive.
        let src = r#"{ "a": "x," }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["a"], serde_json::json!("x,"));
    }

    #[test]
    fn jsonc_unterminated_block_comment_does_not_panic() {
        // The clamped `i` keeps stripping safe even when `*/` is missing; the result is still
        // valid JSON (whitespace before `}` is collapsed, which is fine).
        assert_eq!(strip_jsonc("{}/*").trim(), "{}");
        let v: serde_json::Value =
            serde_json::from_str(&strip_jsonc("{ \"a\": 1 } /* never closed")).unwrap();
        assert_eq!(v["a"], serde_json::json!(1));
    }

    #[test]
    fn parse_json_strict_rejects_comments() {
        // The strict `.json` path must NOT accept comments.
        let err = parse("{ // nope\n}", ConfigFormat::Json).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn parse_jsonc_accepts_comments() {
        let c = parse(
            "{ \"language\": \"ja\", // comment\n }",
            ConfigFormat::Jsonc,
        )
        .unwrap();
        assert_eq!(c.language.as_deref(), Some("ja"));
    }

    #[test]
    fn parse_yaml_roundtrips() {
        let c = parse(
            "language: ja\nrules:\n  sentence-length: false\n",
            ConfigFormat::Yaml,
        )
        .unwrap();
        assert_eq!(c.language.as_deref(), Some("ja"));
        assert!(!c.rules.is_empty());
    }

    #[test]
    fn parse_strips_leading_bom() {
        // A leading BOM must not break any format.
        for (text, fmt) in [
            ("\u{feff}{ \"language\": \"ja\" }", ConfigFormat::Json),
            ("\u{feff}{ \"language\": \"ja\" }", ConfigFormat::Jsonc),
            ("\u{feff}language: ja\n", ConfigFormat::Yaml),
        ] {
            let c = parse(text, fmt).unwrap();
            assert_eq!(c.language.as_deref(), Some("ja"), "format {fmt}");
        }
    }

    #[test]
    fn parse_empty_file_is_default_config_for_all_formats() {
        for fmt in [ConfigFormat::Json, ConfigFormat::Jsonc, ConfigFormat::Yaml] {
            let c = parse("", fmt).unwrap();
            assert_eq!(c, Config::default(), "empty {fmt}");
            let ws = parse("   \n\t", fmt).unwrap();
            assert_eq!(ws, Config::default(), "whitespace {fmt}");
        }
        // A comments-only JSONC document is also the default config.
        assert_eq!(
            parse("// just a comment\n", ConfigFormat::Jsonc).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn yaml_rejects_anchor_alias_bomb() {
        let bomb = "rules:\n  r:\n    options:\n      a: &a [1, 1, 1]\n      b: [*a, *a, *a]\n";
        let err = parse(bomb, ConfigFormat::Yaml).unwrap_err();
        match err {
            ConfigError::Parse { message, .. } => {
                assert!(
                    message.contains("anchors") || message.contains("aliases"),
                    "{message}"
                );
            }
            other => panic!("expected Parse error, got {other:?}"),
        }
        // An alias on its own is rejected too.
        assert!(parse("a: *x\n", ConfigFormat::Yaml).is_err());
    }

    #[test]
    fn yaml_anchor_scan_ignores_non_anchor_ampersand_and_star() {
        // The scanner must NOT flag `&`/`*` inside quotes, comments, or spaced/mid plain
        // scalars (tested directly, since these sample keys would otherwise trip
        // deny_unknown_fields). Includes a double-quote escape (`\"`), a single-quote escape
        // (`''`), and a mid-scalar `&` (preceded by a non-indicator char).
        let ok = "language: ja\n\
                  quoted: \"A & B with an escaped \\\" quote and *star\"\n\
                  single: 'it''s a *.md glob & more'\n\
                  arith: 2 * 3 = 6\n\
                  spaced: a & b\n\
                  midscalar: a&b\n\
                  # & * in a comment\n";
        assert!(reject_yaml_anchors(ok).is_ok());
        // But a real anchor or alias IS flagged.
        assert!(reject_yaml_anchors("x: &anchor 1\n").is_err());
        assert!(reject_yaml_anchors("y: *alias\n").is_err());
        assert!(reject_yaml_anchors("z: [&a 1, *a]\n").is_err());
    }
}
