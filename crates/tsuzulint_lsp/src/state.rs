//! LSP Backend state management.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use tower_lsp::lsp_types::Url;

use tsuzulint_core::Linter;

/// Document content and version cache.
#[derive(Debug)]
pub(crate) struct DocumentData {
    pub text: String,
    pub version: i32,
}

/// Shared backend state.
pub(crate) struct BackendState {
    /// Document contents cache.
    pub documents: RwLock<HashMap<Url, DocumentData>>,
    /// Linter instance (may be None if initialization failed).
    pub linter: RwLock<Option<Linter>>,
    /// Workspace root path.
    pub workspace_root: RwLock<Option<std::path::PathBuf>>,
}

impl fmt::Debug for BackendState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackendState")
            .field("documents", &"<HashMap<Url, DocumentData>>")
            .field("linter", &"<Option<Linter>>")
            .field("workspace_root", &self.workspace_root)
            .finish()
    }
}

impl BackendState {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self {
            documents: RwLock::new(HashMap::new()),
            linter: RwLock::new(None),
            workspace_root: RwLock::new(None),
        }
    }

    /// Creates a new state with a pre-initialized linter.
    pub fn with_linter(linter: Option<Linter>) -> Self {
        Self {
            documents: RwLock::new(HashMap::new()),
            linter: RwLock::new(linter),
            workspace_root: RwLock::new(None),
        }
    }
}

impl Default for BackendState {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for shared state.
pub type SharedState = Arc<BackendState>;
