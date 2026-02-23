use crate::error::LinterError;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

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

        let mut glob_builder = GlobSetBuilder::new();
        let mut has_globs = false;

        for pattern in patterns {
            let path = Path::new(pattern);
            if path
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_file())
            {
                if let Ok(abs_path) = path.canonicalize() {
                    if self.should_ignore(&abs_path) {
                        continue;
                    }

                    files.push(abs_path);
                }
            } else {
                let glob = Glob::new(pattern).map_err(|e| {
                    LinterError::config(format!("Invalid pattern '{}': {}", pattern, e))
                })?;
                glob_builder.add(glob);
                has_globs = true;
            }
        }

        if has_globs {
            let glob_set = glob_builder
                .build()
                .map_err(|e| LinterError::config(format!("Failed to build globset: {}", e)))?;

            for entry in WalkDir::new(base_dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() && glob_set.is_match(path) {
                    if self.should_ignore(path) {
                        continue;
                    }

                    files.push(path.to_path_buf());
                }
            }
        }

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
        assert!(finder.include_globs.is_some());
        assert!(finder.exclude_globs.is_none());
    }

    #[test]
    fn test_file_finder_with_exclude_patterns() {
        let finder = FileFinder::new(&[], &["**/node_modules/**".to_string()]).unwrap();
        assert!(finder.include_globs.is_none());
        assert!(finder.exclude_globs.is_some());
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

        let result = finder.discover_files(&["[invalid-glob".to_string()], Path::new("."));
        assert!(result.is_err());
    }
}
