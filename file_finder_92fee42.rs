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
            if path.exists() && path.is_file() {
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
