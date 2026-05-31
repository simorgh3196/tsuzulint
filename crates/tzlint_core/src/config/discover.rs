//! Upward config discovery over the [`Host`](crate::io::Host) boundary.

use std::fmt;
use std::path::{Path, PathBuf};

use super::format::{self, ConfigFormat};
use super::{Config, ConfigError};
use crate::io::{Host, MAX_CONFIG};

/// Candidate config file names, highest priority first, paired with their format.
const CANDIDATES: &[(&str, ConfigFormat)] = &[
    (".tzlintrc.jsonc", ConfigFormat::Jsonc),
    (".tzlintrc.json", ConfigFormat::Json),
    (".tzlintrc.yaml", ConfigFormat::Yaml),
    (".tzlintrc.yml", ConfigFormat::Yaml),
    (".tzlintrc", ConfigFormat::Jsonc),
];

/// A lower-priority config candidate that was shadowed (and therefore ignored) because a
/// higher-priority candidate existed in the same directory.
///
/// Returned as structured data (rather than a pre-rendered sentence) so embedders can attach
/// the paths to their own diagnostics and localize the wording; [`Display`](fmt::Display)
/// provides a default English rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowedCandidate {
    /// The candidate that was ignored.
    pub ignored: PathBuf,
    /// The higher-priority candidate that was loaded instead.
    pub winner: PathBuf,
}

impl fmt::Display for ShadowedCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ignoring `{}`: shadowed by higher-priority `{}`",
            self.ignored.display(),
            self.winner.display()
        )
    }
}

/// A config found by [`discover`], plus any warnings about ignored co-located candidates.
#[derive(Debug, Clone)]
pub struct DiscoveredConfig {
    /// The file the config was loaded from.
    pub path: PathBuf,
    /// The format it was parsed as.
    pub format: ConfigFormat,
    /// The parsed config.
    pub config: Config,
    /// Lower-priority candidates in the same directory that were shadowed (and ignored). Empty
    /// in the common case.
    pub warnings: Vec<ShadowedCandidate>,
}

/// Walk upward from `start_dir`, returning the first config found.
///
/// At each directory (starting with `start_dir` itself), candidate files are probed in
/// priority order via [`Host::exists`]. The first directory holding any candidate wins; its
/// highest-priority candidate is read (capped at [`MAX_CONFIG`]) and parsed, and lower-priority
/// candidates in that same directory become [`warnings`](DiscoveredConfig::warnings). Returns
/// `Ok(None)` if no candidate exists in any ancestor directory.
///
/// Discovery is best-effort across a small TOCTOU window: a candidate can vanish between the
/// `exists` probe and the read (surfacing as [`ConfigError::Io`]) or be swapped for a symlink
/// — the read follows symlinks but stays bounded by [`MAX_CONFIG`], so there is no unbounded
/// read.
pub fn discover(
    host: &dyn Host,
    start_dir: &Path,
) -> Result<Option<DiscoveredConfig>, ConfigError> {
    for dir in start_dir.ancestors() {
        let mut present = CANDIDATES
            .iter()
            .filter(|(name, _)| host.exists(&dir.join(name)));

        let Some(&(winner_name, format)) = present.next() else {
            continue; // nothing here; try the parent directory
        };
        let path = dir.join(winner_name);

        // Remaining candidates in this directory are shadowed by the winner.
        let warnings: Vec<ShadowedCandidate> = present
            .map(|&(name, _)| ShadowedCandidate {
                ignored: dir.join(name),
                winner: path.clone(),
            })
            .collect();

        let text = host.read_to_string(&path, MAX_CONFIG)?;
        let config = format::parse(&text, format)?;
        return Ok(Some(DiscoveredConfig {
            path,
            format,
            config,
            warnings,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::IoError;
    use std::collections::HashMap;

    /// A read-only, in-memory [`Host`] for hermetic discovery tests: only registered paths
    /// exist, so the upward walk terminates deterministically (no real filesystem).
    struct MockHost {
        files: HashMap<PathBuf, String>,
    }

    impl MockHost {
        fn new(files: &[(&str, &str)]) -> Self {
            MockHost {
                files: files
                    .iter()
                    .map(|(p, c)| (PathBuf::from(p), (*c).to_string()))
                    .collect(),
            }
        }
    }

    impl Host for MockHost {
        fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
            match self.files.get(path) {
                Some(s) if s.len() > limit => Err(IoError::TooLarge { limit }),
                Some(s) => Ok(s.clone()),
                None => Err(IoError::NotFound),
            }
        }
        fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<(), IoError> {
            Err(IoError::Other("read-only mock host".into()))
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }
    }

    #[test]
    fn finds_config_in_start_dir() {
        let host = MockHost::new(&[("/proj/.tzlintrc.json", r#"{ "language": "ja" }"#)]);
        let found = discover(&host, Path::new("/proj")).unwrap().unwrap();
        assert_eq!(found.path, PathBuf::from("/proj/.tzlintrc.json"));
        assert_eq!(found.format, ConfigFormat::Json);
        assert_eq!(found.config.language.as_deref(), Some("ja"));
        assert!(found.warnings.is_empty());
    }

    #[test]
    fn walks_upward_to_an_ancestor() {
        let host = MockHost::new(&[("/proj/.tzlintrc.yaml", "language: ja\n")]);
        // Start two directories deep; the config lives at /proj.
        let found = discover(&host, Path::new("/proj/src/docs"))
            .unwrap()
            .unwrap();
        assert_eq!(found.path, PathBuf::from("/proj/.tzlintrc.yaml"));
        assert_eq!(found.format, ConfigFormat::Yaml);
    }

    #[test]
    fn highest_priority_wins_and_others_warn() {
        let host = MockHost::new(&[
            (".tzlintrc.jsonc", "{}"), // relative names → ancestors() yields "" then nothing
            (".tzlintrc.json", "{}"),
            (".tzlintrc", "{}"),
        ]);
        // Use a relative start dir whose only ancestor is "" (current dir); join("name")
        // yields the bare candidate names registered above.
        let found = discover(&host, Path::new("")).unwrap().unwrap();
        assert_eq!(found.path, PathBuf::from(".tzlintrc.jsonc"));
        assert_eq!(found.format, ConfigFormat::Jsonc);
        // The two shadowed candidates are reported as structured data, each pointing at the
        // winner.
        assert_eq!(found.warnings.len(), 2);
        assert!(found.warnings.iter().all(|w| w.winner == found.path));
        let ignored: Vec<&PathBuf> = found.warnings.iter().map(|w| &w.ignored).collect();
        assert!(ignored.contains(&&PathBuf::from(".tzlintrc.json")));
        assert!(ignored.contains(&&PathBuf::from(".tzlintrc")));
        // Display renders both paths.
        assert!(found.warnings[0].to_string().contains("shadowed by"));
    }

    #[test]
    fn nearer_directory_shadows_farther_one() {
        let host = MockHost::new(&[
            ("/proj/.tzlintrc.json", r#"{ "language": "outer" }"#),
            ("/proj/src/.tzlintrc.json", r#"{ "language": "inner" }"#),
        ]);
        let found = discover(&host, Path::new("/proj/src")).unwrap().unwrap();
        assert_eq!(found.path, PathBuf::from("/proj/src/.tzlintrc.json"));
        assert_eq!(found.config.language.as_deref(), Some("inner"));
    }

    #[test]
    fn returns_none_when_nothing_found() {
        let host = MockHost::new(&[]);
        assert!(discover(&host, Path::new("/proj/src")).unwrap().is_none());
    }

    #[test]
    fn parse_error_propagates() {
        let host = MockHost::new(&[("/proj/.tzlintrc.json", "{ not valid json ")]);
        let err = discover(&host, Path::new("/proj")).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn read_error_propagates() {
        // Candidate exists but exceeds the config size cap → the IoError surfaces.
        let big = "x".repeat(MAX_CONFIG + 1);
        let host = MockHost::new(&[("/proj/.tzlintrc.json", &big)]);
        let err = discover(&host, Path::new("/proj")).unwrap_err();
        assert!(matches!(err, ConfigError::Io(IoError::TooLarge { .. })));
    }
}
