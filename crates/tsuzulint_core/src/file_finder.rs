use crate::error::LinterError;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

pub struct FileFinder {
    include_globs: Option<GlobSet>,
    exclude_globs: Option<GlobSet>,
}

impl FileFinder {
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self, LinterError> {
        let include_globs = Self::build_globset(include)?;
        let exclude_globs = Self::build_globset(exclude)?;

        Ok(Self {
            include_globs,
            exclude_globs,
        })
    }

    fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>, LinterError> {
        if patterns.is_empty() {
            return Ok(None);
        }

        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = Glob::new(pattern)
                .map_err(|e| LinterError::config(format!("Invalid glob pattern: {}", e)))?;
            builder.add(glob);
        }

        let globset = builder
            .build()
            .map_err(|e| LinterError::config(format!("Failed to build globset: {}", e)))?;

        Ok(Some(globset))
    }

    /// Checks if a file path should be ignored based on include/exclude patterns.
    pub fn should_ignore(&self, path: &Path) -> bool {
        if self
            .exclude_globs
            .as_ref()
            .is_some_and(|excludes| excludes.is_match(path))
        {
            return true;
        }

        if self
            .include_globs
            .as_ref()
            .is_some_and(|includes| !includes.is_match(path))
        {
            return true;
        }

        false
    }

    pub fn discover_files(
        &self,
        patterns: &[String],
        base_dir: &Path,
    ) -> Result<Vec<PathBuf>, LinterError> {
        let mut files = Vec::new();
        let mut glob_patterns = Vec::new();

        // 1. Handle explicit file paths (canonicalize and check ignore)
        // 2. Collect glob patterns for the walker
        for pattern in patterns {
            let path = base_dir.join(pattern);
            if path
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_file())
            {
                match path.canonicalize() {
                    Ok(abs_path) => {
                        if self.should_ignore(&abs_path) {
                            continue;
                        }
                        files.push(abs_path);
                    }
                    Err(e) => {
                        warn!(path = ?path, error = %e, "Failed to canonicalize direct file path");
                        continue;
                    }
                }
            } else {
                glob_patterns.push(pattern);
            }
        }

        if glob_patterns.is_empty() {
            files.sort();
            files.dedup();
            return Ok(files);
        }

        // Build GlobSet for CLI patterns (to filter results)
        // We do NOT use overrides because whitelist overrides bypass .gitignore
        let mut builder = GlobSetBuilder::new();
        for pattern in glob_patterns {
            let glob = Glob::new(pattern).map_err(|e| {
                LinterError::config(format!("Invalid glob pattern '{}': {}", pattern, e))
            })?;
            builder.add(glob);
        }
        let cli_glob_set = builder
            .build()
            .map_err(|e| LinterError::config(format!("Failed to build globset: {}", e)))?;

        // Shared results vector protected by Mutex
        let found_files = Arc::new(Mutex::new(files));
        let found_files_clone = found_files.clone();

        // GlobSets are cheap to clone (Arc internally)
        let include_globs = self.include_globs.clone();
        let exclude_globs = self.exclude_globs.clone();
        let cli_glob_set = cli_glob_set.clone();

        WalkBuilder::new(base_dir)
            .follow_links(false)
            .hidden(false)
            .ignore(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(false)
            .build_parallel()
            .run(move || {
                let found_files = found_files_clone.clone();
                let include_globs = include_globs.clone();
                let exclude_globs = exclude_globs.clone();
                let cli_glob_set = cli_glob_set.clone();

                Box::new(move |entry| {
                    match entry {
                        Ok(entry) => {
                            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                                let path = entry.path();

                                // Check CLI pattern match first (fastest)
                                if cli_glob_set.is_match(path) {
                                    match path.canonicalize() {
                                        Ok(abs_path) => {
                                            // Re-implement should_ignore logic here
                                        if exclude_globs
                                                .as_ref()
                                                .is_some_and(|excludes| excludes.is_match(&abs_path))
                                            {
                                            return ignore::WalkState::Continue;
                                        }

                                        if include_globs
                                                .as_ref()
                                                .is_some_and(|includes| !includes.is_match(&abs_path))
                                            {
                                            return ignore::WalkState::Continue;
                                        }

                                        if let Ok(mut lock) = found_files.lock() {
                                            lock.push(abs_path);
                                            }
                                        }
                                        Err(e) => {
                                            debug!(path = ?path, error = %e, "Failed to canonicalize discovered file path");
                                        }
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            debug!(error = ?err, "Walk error scanning base_dir");
                        }
                    }
                    ignore::WalkState::Continue
                })
            });

        let mut files = {
            let mut lock = found_files.lock().map_err(|_| {
                LinterError::Internal("Failed to lock found files mutex".to_string())
            })?;
            std::mem::take(&mut *lock)
        };

        files.sort();
        files.dedup();

        info!("Discovered {} files to lint", files.len());
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_build_globset() {
        let patterns = vec!["**/*.md".to_string(), "*.txt".to_string()];
        let result = FileFinder::build_globset(&patterns);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_build_globset_empty() {
        let patterns: Vec<String> = vec![];
        let result = FileFinder::build_globset(&patterns);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_globset_invalid_pattern() {
        let patterns = vec!["[invalid".to_string()];
        let result = FileFinder::build_globset(&patterns);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_finder_with_include_patterns() {
        let finder = FileFinder::new(&["**/*.md".to_string()], &[]).unwrap();
        // Returns false (do not ignore) for matched inclusive files
        assert!(!finder.should_ignore(Path::new("README.md")));
        assert!(!finder.should_ignore(Path::new("docs/setup.md")));
        // Returns true (ignore) for non-matched inclusive files
        assert!(finder.should_ignore(Path::new("src/lib.rs")));
    }

    #[test]
    fn test_file_finder_with_exclude_patterns() {
        let finder = FileFinder::new(&[], &["**/node_modules/**".to_string()]).unwrap();
        // Returns true (ignore) for excluded files
        assert!(finder.should_ignore(Path::new("node_modules/pkg/index.js")));
        // Returns false (do not ignore) for all other files
        assert!(!finder.should_ignore(Path::new("src/code.js")));
    }

    #[test]
    fn test_build_globset_multiple_patterns() {
        let patterns = vec![
            "**/*.md".to_string(),
            "**/*.txt".to_string(),
            "docs/**/*".to_string(),
        ];
        let result = FileFinder::build_globset(&patterns);
        assert!(result.is_ok());

        let globset = result.unwrap().unwrap();
        assert!(globset.is_match("file.md"));
        assert!(globset.is_match("dir/file.txt"));
        assert!(globset.is_match("docs/readme.md"));
    }

    #[test]
    fn test_discover_files_respects_exclude() {
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.md");
        let node_modules = temp_dir.path().join("node_modules");
        fs::create_dir(&node_modules).unwrap();
        let excluded_file = node_modules.join("excluded.md");

        fs::write(&test_file, "# Test").unwrap();
        fs::write(&excluded_file, "# Excluded").unwrap();

        let finder = FileFinder::new(
            &["**/*.md".to_string()],
            &["**/node_modules/**".to_string()],
        )
        .unwrap();

        let files = finder
            .discover_files(&["**/*.md".to_string()], temp_dir.path())
            .unwrap();

        assert!(files.iter().any(|f| f.ends_with("test.md")));
        assert!(
            !files
                .iter()
                .any(|f| f.to_string_lossy().contains("node_modules"))
        );
    }

    #[test]
    fn test_discover_files_respects_include() {
        let temp_dir = tempdir().unwrap();
        let md_file = temp_dir.path().join("test.md");
        let txt_file = temp_dir.path().join("test.txt");

        fs::write(&md_file, "# Test").unwrap();
        fs::write(&txt_file, "Test").unwrap();

        let finder = FileFinder::new(&["**/*.md".to_string()], &[]).unwrap();

        let files = finder
            .discover_files(&["**/*".to_string()], temp_dir.path())
            .unwrap();

        assert!(files.iter().any(|f| f.ends_with("test.md")));
        assert!(!files.iter().any(|f| f.ends_with("test.txt")));
    }

    #[test]
    fn test_discover_files_exclude_takes_priority_over_include() {
        let temp_dir = tempdir().unwrap();
        let included_file = temp_dir.path().join("docs").join("readme.md");
        let excluded_file = temp_dir
            .path()
            .join("node_modules")
            .join("docs")
            .join("internal.md");

        fs::create_dir_all(included_file.parent().unwrap()).unwrap();
        fs::create_dir_all(excluded_file.parent().unwrap()).unwrap();
        fs::write(&included_file, "# Readme").unwrap();
        fs::write(&excluded_file, "# Internal").unwrap();

        let finder = FileFinder::new(
            &["**/*.md".to_string()],
            &["**/node_modules/**".to_string()],
        )
        .unwrap();

        let files = finder
            .discover_files(&["**/*.md".to_string()], temp_dir.path())
            .unwrap();

        assert!(
            files.iter().any(|f| f.ends_with("readme.md")),
            "included file should be discovered"
        );
        assert!(
            !files
                .iter()
                .any(|f| f.to_string_lossy().contains("node_modules")),
            "excluded file should not be discovered even though it matches include glob"
        );
    }

    #[test]
    fn test_discover_files_deduplicates() {
        let temp_dir = tempdir().unwrap();
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test").unwrap();

        let finder = FileFinder::new(&[], &[]).unwrap();

        let files = finder
            .discover_files(&["*.md".to_string(), "*.md".to_string()], temp_dir.path())
            .unwrap();

        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_discover_files_multiple_glob_patterns() {
        let temp_dir = tempdir().unwrap();
        let md_file = temp_dir.path().join("test.md");
        let txt_file = temp_dir.path().join("test.txt");
        let rs_file = temp_dir.path().join("test.rs");

        fs::write(&md_file, "# Test").unwrap();
        fs::write(&txt_file, "Test").unwrap();
        fs::write(&rs_file, "fn main() {}").unwrap();

        let finder = FileFinder::new(&[], &[]).unwrap();

        let files = finder
            .discover_files(
                &["**/*.md".to_string(), "**/*.txt".to_string()],
                temp_dir.path(),
            )
            .unwrap();

        assert!(
            files.iter().any(|f| f.ends_with("test.md")),
            "Should find .md file"
        );
        assert!(
            files.iter().any(|f| f.ends_with("test.txt")),
            "Should find .txt file"
        );
        assert!(
            !files.iter().any(|f| f.ends_with("test.rs")),
            "Should not find .rs file"
        );
        assert_eq!(files.len(), 2, "Should find exactly 2 files");
    }

    #[test]
    fn test_discover_files_invalid_glob() {
        let finder = FileFinder::new(&[], &[]).unwrap();
        // The previous test expected error. With ignore::overrides::OverrideBuilder, it might also error.
        let result = finder.discover_files(&["[invalid-glob".to_string()], Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    #[cfg(unix)]
    fn test_discover_files_ignores_symlinks() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempdir().unwrap();
        let target_file = temp_dir.path().join("target.md");
        let link_file = temp_dir.path().join("link.md");

        fs::write(&target_file, "# Target").unwrap();
        symlink(&target_file, &link_file).unwrap();

        let finder = FileFinder::new(&[], &[]).unwrap();

        let files = finder
            .discover_files(&["*.md".to_string()], temp_dir.path())
            .unwrap();

        // Should only contain target.md, NOT link.md
        // WalkBuilder follows links: false by default
        assert_eq!(
            files.len(),
            1,
            "Should ignore symlinks, but found: {:?}",
            files
        );
        assert!(files.iter().any(|f| f.ends_with("target.md")));
        assert!(!files.iter().any(|f| f.ends_with("link.md")));
    }

    #[test]
    fn test_discover_files_respects_gitignore() {
        use std::io::Write;

        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();

        // Initialize git repo so .gitignore works (ignore crate requires it usually, or at least a .gitignore file)
        // Actually ignore crate works without .git directory if .gitignore is present, but let's be safe.
        // Wait, WalkBuilder defaults to respecting .gitignore.
        use std::process::Command;
        let _ = Command::new("git").arg("init").current_dir(root).output();

        let ignored_file = root.join("ignored.md");
        let included_file = root.join("included.md");
        let gitignore = root.join(".gitignore");

        fs::write(&ignored_file, "ignored").unwrap();
        fs::write(&included_file, "included").unwrap();

        let mut f = fs::File::create(&gitignore).unwrap();
        writeln!(f, "ignored.md").unwrap();

        let finder = FileFinder::new(&[], &[]).unwrap();

        let files = finder.discover_files(&["*.md".to_string()], root).unwrap();

        assert!(files.iter().any(|f| f.ends_with("included.md")));
        assert!(
            !files.iter().any(|f| f.ends_with("ignored.md")),
            "Should respect .gitignore"
        );
    }
}
