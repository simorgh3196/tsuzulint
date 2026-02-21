//! Parallel file walker using the `ignore` crate.
//!
//! This module provides high-performance file discovery with:
//! - Automatic `.gitignore` support
//! - Hidden file filtering
//! - Parallel traversal using `ignore::WalkBuilder`
//! - Thread-safe result collection via crossbeam channels

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use ignore::{DirEntry, Error, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use tracing::{debug, info};

/// Configuration for parallel file walking.
#[derive(Debug, Clone)]
pub struct WalkConfig {
    /// Whether to respect `.gitignore` files.
    /// Default: true
    pub respect_gitignore: bool,
    /// Whether to include hidden files (files starting with `.`).
    /// Default: false (excludes hidden files)
    pub include_hidden: bool,
    /// Number of threads to use for parallel walking.
    /// Default: 0 (uses all available CPUs)
    pub threads: usize,
    /// Whether to follow symbolic links.
    /// Default: false
    pub follow_links: bool,
    /// Maximum directory depth to traverse.
    /// Default: None (no limit)
    pub max_depth: Option<usize>,
    /// Glob patterns to include (e.g., `*.md`, `**/*.txt`).
    pub include_patterns: Vec<String>,
    /// Glob patterns to exclude.
    pub exclude_patterns: Vec<String>,
}

impl Default for WalkConfig {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            include_hidden: false,
            threads: 0,
            follow_links: false,
            max_depth: None,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
        }
    }
}

impl WalkConfig {
    /// Creates a new `WalkConfig` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables `.gitignore` support.
    pub fn respect_gitignore(mut self, yes: bool) -> Self {
        self.respect_gitignore = yes;
        self
    }

    /// Enables or disables hidden file inclusion.
    pub fn include_hidden(mut self, yes: bool) -> Self {
        self.include_hidden = yes;
        self
    }

    /// Sets the number of threads for parallel walking.
    pub fn threads(mut self, n: usize) -> Self {
        self.threads = n;
        self
    }

    /// Sets whether to follow symbolic links.
    pub fn follow_links(mut self, yes: bool) -> Self {
        self.follow_links = yes;
        self
    }

    /// Sets the maximum directory depth.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Adds an include glob pattern.
    pub fn include(mut self, pattern: impl Into<String>) -> Self {
        self.include_patterns.push(pattern.into());
        self
    }

    /// Adds an exclude glob pattern.
    pub fn exclude(mut self, pattern: impl Into<String>) -> Self {
        self.exclude_patterns.push(pattern.into());
        self
    }
}

/// Parallel file walker using `ignore::WalkBuilder`.
///
/// This walker leverages the `ignore` crate's parallel traversal
/// capabilities for high-performance file discovery.
pub struct ParallelWalker {
    config: WalkConfig,
}

impl ParallelWalker {
    /// Creates a new `ParallelWalker` with the given configuration.
    pub fn new(config: WalkConfig) -> Self {
        Self { config }
    }

    /// Creates a new `ParallelWalker` with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(WalkConfig::default())
    }

    /// Walks the given paths in parallel and returns all discovered files.
    ///
    /// This method uses `ignore::WalkBuilder::build_parallel()` for
    /// high-performance parallel traversal.
    pub fn walk(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        if paths.is_empty() {
            return Vec::new();
        }

        let (tx, rx) = crossbeam_channel::bounded::<PathBuf>(1024);
        let file_count = Arc::new(AtomicUsize::new(0));
        let error_count = Arc::new(AtomicUsize::new(0));

        // Build the walker for the first path
        let mut builder = WalkBuilder::new(&paths[0]);

        // Add additional paths
        for path in &paths[1..] {
            builder.add(path);
        }

        // Configure the builder
        builder
            .git_ignore(self.config.respect_gitignore)
            .git_global(self.config.respect_gitignore)
            .git_exclude(self.config.respect_gitignore)
            .hidden(!self.config.include_hidden)
            .follow_links(self.config.follow_links)
            .threads(self.config.threads);

        if let Some(depth) = self.config.max_depth {
            builder.max_depth(Some(depth));
        }

        // Build glob matcher for include/exclude filtering
        // GlobMatcher is immutable after construction, no Mutex needed
        let glob_matcher = Arc::new(GlobMatcher::new(
            &self.config.include_patterns,
            &self.config.exclude_patterns,
        ));

        // Spawn receiver thread to avoid deadlock
        // With bounded channel, if we collect after visit() completes,
        // workers would block when channel is full (1024+ files)
        let receiver_handle = Self::spawn_receiver(rx, file_count.clone(), error_count.clone());

        // Create visitor builder
        let mut visitor_builder = FileVisitorBuilder {
            tx,
            glob_matcher,
            file_count: file_count.clone(),
            error_count: error_count.clone(),
        };

        // Create the parallel walker and run
        let walker = builder.build_parallel();
        walker.visit(&mut visitor_builder);

        // Drop the sender to close the channel
        drop(visitor_builder);

        // Wait for receiver thread and get results
        let results = receiver_handle.join().unwrap_or_default();

        let count = file_count.load(Ordering::Relaxed);
        let errors = error_count.load(Ordering::Relaxed);

        info!(
            "ParallelWalker: discovered {} files ({} errors)",
            count, errors
        );

        results
    }

    /// Spawns a receiver thread to collect results concurrently.
    /// This prevents deadlock when the bounded channel fills up.
    fn spawn_receiver(
        rx: Receiver<PathBuf>,
        file_count: Arc<AtomicUsize>,
        error_count: Arc<AtomicUsize>,
    ) -> thread::JoinHandle<Vec<PathBuf>> {
        thread::spawn(move || {
            let results: Vec<PathBuf> = rx.iter().collect();
            let count = file_count.load(Ordering::Relaxed);
            let errors = error_count.load(Ordering::Relaxed);
            debug!("Receiver collected {} files ({} errors)", count, errors);
            results
        })
    }

    /// Walks a single path in parallel.
    pub fn walk_path(&self, path: impl AsRef<Path>) -> Vec<PathBuf> {
        self.walk(&[path.as_ref().to_path_buf()])
    }
}

/// Visitor builder for parallel walking.
struct FileVisitorBuilder {
    tx: Sender<PathBuf>,
    glob_matcher: Arc<GlobMatcher>,
    file_count: Arc<AtomicUsize>,
    error_count: Arc<AtomicUsize>,
}

impl<'s> ParallelVisitorBuilder<'s> for FileVisitorBuilder {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(FileVisitor {
            tx: self.tx.clone(),
            glob_matcher: Arc::clone(&self.glob_matcher),
            file_count: self.file_count.clone(),
            error_count: self.error_count.clone(),
        })
    }
}

/// Per-thread visitor for parallel walking.
struct FileVisitor {
    tx: Sender<PathBuf>,
    glob_matcher: Arc<GlobMatcher>,
    file_count: Arc<AtomicUsize>,
    error_count: Arc<AtomicUsize>,
}

impl ParallelVisitor for FileVisitor {
    fn visit(&mut self, entry: Result<DirEntry, Error>) -> WalkState {
        match entry {
            Ok(dir_entry) => {
                if dir_entry.file_type().is_some_and(|ft| ft.is_file()) {
                    let path = dir_entry.path();

                    if self.glob_matcher.should_include(path)
                        && self.tx.send(path.to_path_buf()).is_ok()
                    {
                        self.file_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(e) => {
                self.error_count.fetch_add(1, Ordering::Relaxed);
                debug!("Walk error: {}", e);
            }
        }
        WalkState::Continue
    }
}

/// Glob pattern matcher for filtering files.
struct GlobMatcher {
    include_set: Option<globset::GlobSet>,
    exclude_set: Option<globset::GlobSet>,
}

impl GlobMatcher {
    fn new(include_patterns: &[String], exclude_patterns: &[String]) -> Self {
        let include_set = Self::build_globset(include_patterns, "include");
        let exclude_set = Self::build_globset(exclude_patterns, "exclude");

        Self {
            include_set,
            exclude_set,
        }
    }

    /// Builds a GlobSet from a list of patterns.
    ///
    /// Returns `None` if the pattern list is empty.
    /// Logs a warning for any invalid patterns.
    fn build_globset(patterns: &[String], name: &str) -> Option<globset::GlobSet> {
        use globset::{Glob, GlobSetBuilder};

        if patterns.is_empty() {
            return None;
        }

        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            match Glob::new(pattern) {
                Ok(glob) => {
                    builder.add(glob);
                }
                Err(e) => {
                    tracing::warn!("Invalid {} glob pattern {:?}: {}", name, pattern, e);
                }
            }
        }
        match builder.build() {
            Ok(set) => Some(set),
            Err(e) => {
                tracing::warn!("Failed to build {} glob set: {}", name, e);
                None
            }
        }
    }

    fn should_include(&self, path: &Path) -> bool {
        // Check exclude first
        if let Some(ref exclude) = self.exclude_set
            && exclude.is_match(path)
        {
            return false;
        }

        // Check include
        if let Some(ref include) = self.include_set
            && !include.is_match(path)
        {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    fn create_test_tree() -> TempDir {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Initialize a git repository so .gitignore is respected
        let _ = Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output();

        // Create files
        fs::write(root.join("file1.md"), "# File 1").unwrap();
        fs::write(root.join("file2.txt"), "File 2").unwrap();
        fs::write(root.join("file3.md"), "# File 3").unwrap();

        // Create hidden file
        fs::write(root.join(".hidden.md"), "# Hidden").unwrap();

        // Create subdirectory
        let subdir = root.join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file4.md"), "# File 4").unwrap();
        fs::write(subdir.join("file5.txt"), "File 5").unwrap();

        // Create node_modules (should be ignored by gitignore)
        let node_modules = root.join("node_modules");
        fs::create_dir(&node_modules).unwrap();
        fs::write(node_modules.join("package.md"), "# Package").unwrap();

        // Create .gitignore
        let mut gitignore = fs::File::create(root.join(".gitignore")).unwrap();
        writeln!(gitignore, "node_modules/").unwrap();
        writeln!(gitignore, "*.tmp").unwrap();

        temp
    }

    #[test]
    fn test_walk_config_default() {
        let config = WalkConfig::default();
        assert!(config.respect_gitignore);
        assert!(!config.include_hidden);
        assert_eq!(config.threads, 0);
        assert!(!config.follow_links);
        assert!(config.max_depth.is_none());
    }

    #[test]
    fn test_walk_config_builder() {
        let config = WalkConfig::new()
            .respect_gitignore(false)
            .include_hidden(true)
            .threads(4)
            .follow_links(true)
            .max_depth(10)
            .include("*.md")
            .exclude("**/target/**");

        assert!(!config.respect_gitignore);
        assert!(config.include_hidden);
        assert_eq!(config.threads, 4);
        assert!(config.follow_links);
        assert_eq!(config.max_depth, Some(10));
        assert_eq!(config.include_patterns, vec!["*.md"]);
        assert_eq!(config.exclude_patterns, vec!["**/target/**"]);
    }

    #[test]
    fn test_parallel_walker_respects_gitignore() {
        let temp = create_test_tree();
        let root = temp.path();

        let walker = ParallelWalker::new(WalkConfig::new().respect_gitignore(true));

        let files = walker.walk_path(root);

        // node_modules should be ignored
        assert!(
            !files
                .iter()
                .any(|f| f.to_string_lossy().contains("node_modules"))
        );

        // Regular files should be found
        assert!(files.iter().any(|f| f.ends_with("file1.md")));
        assert!(files.iter().any(|f| f.ends_with("file4.md")));
    }

    #[test]
    fn test_parallel_walker_ignores_gitignore_when_disabled() {
        let temp = create_test_tree();
        let root = temp.path();

        let walker = ParallelWalker::new(WalkConfig::new().respect_gitignore(false));

        let files = walker.walk_path(root);

        // node_modules should NOT be ignored
        assert!(
            files
                .iter()
                .any(|f| f.to_string_lossy().contains("node_modules"))
        );
    }

    #[test]
    fn test_parallel_walker_excludes_hidden_files() {
        let temp = create_test_tree();
        let root = temp.path();

        let walker = ParallelWalker::new(WalkConfig::new().include_hidden(false));

        let files = walker.walk_path(root);

        // .hidden.md should be excluded
        assert!(!files.iter().any(|f| {
            f.file_name()
                .is_some_and(|n| n.to_string_lossy() == ".hidden.md")
        }));
    }

    #[test]
    fn test_parallel_walker_includes_hidden_files_when_enabled() {
        let temp = create_test_tree();
        let root = temp.path();

        let walker = ParallelWalker::new(WalkConfig::new().include_hidden(true));

        let files = walker.walk_path(root);

        // .hidden.md should be included
        assert!(files.iter().any(|f| {
            f.file_name()
                .is_some_and(|n| n.to_string_lossy() == ".hidden.md")
        }));
    }

    #[test]
    fn test_parallel_walker_max_depth() {
        let temp = create_test_tree();
        let root = temp.path();

        let walker = ParallelWalker::new(WalkConfig::new().max_depth(1));

        let files = walker.walk_path(root);

        // Only top-level files should be found (max_depth(1) = root + direct children)
        for file in &files {
            assert!(
                file.parent() == Some(root)
                    || file.parent() == Some(root.canonicalize().unwrap().as_path())
            );
        }
    }

    #[test]
    fn test_parallel_walker_empty_paths() {
        let walker = ParallelWalker::with_defaults();
        let files = walker.walk(&[]);
        assert!(files.is_empty());
    }

    #[test]
    fn test_parallel_walker_single_file() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("test.md");
        fs::write(&file, "# Test").unwrap();

        let walker = ParallelWalker::with_defaults();
        let files = walker.walk_path(&file);

        // Single file should be found
        assert!(!files.is_empty());
        assert!(files.iter().any(|f| f.ends_with("test.md")));
    }

    #[test]
    fn test_glob_matcher_include() {
        let matcher = GlobMatcher::new(&["*.md".to_string()], &[]);

        assert!(matcher.should_include(Path::new("test.md")));
        assert!(matcher.should_include(Path::new("subdir/test.md")));
        assert!(!matcher.should_include(Path::new("test.txt")));
    }

    #[test]
    fn test_glob_matcher_exclude() {
        let matcher = GlobMatcher::new(&[], &["**/target/**".to_string()]);

        assert!(matcher.should_include(Path::new("src/main.rs")));
        assert!(!matcher.should_include(Path::new("target/debug/main.rs")));
    }

    #[test]
    fn test_glob_matcher_include_and_exclude() {
        let matcher = GlobMatcher::new(&["*.md".to_string()], &["**/node_modules/**".to_string()]);

        assert!(matcher.should_include(Path::new("README.md")));
        assert!(!matcher.should_include(Path::new("node_modules/pkg/README.md")));
        assert!(!matcher.should_include(Path::new("src/main.rs")));
    }
}
