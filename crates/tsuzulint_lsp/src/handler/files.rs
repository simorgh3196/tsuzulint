//! Watched files handler.

use tower_lsp::lsp_types::*;
use tracing::{debug, info};

use crate::config::reload_config;
use crate::state::BackendState;

/// Handles the `workspace/didChangeWatchedFiles` notification.
pub async fn handle_did_change_watched_files(
    state: &BackendState,
    params: DidChangeWatchedFilesParams,
) {
    debug!("Watched files changed: {:?}", params.changes);

    let config_changed = params.changes.iter().any(|change| {
        let path = change.uri.path();
        tsuzulint_core::LinterConfig::CONFIG_FILES
            .iter()
            .any(|name| path.ends_with(name))
    });

    if config_changed {
        info!("Configuration file changed, reloading...");
        reload_config(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BackendState;
    use std::fs;
    use tempfile::tempdir;
    use tower_lsp::lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Url};

    #[tokio::test]
    async fn test_handle_did_change_watched_files_config_change() {
        let state = BackendState::new();
        let temp = tempdir().unwrap();
        let config_path = temp.path().join(".tsuzulint.json");
        fs::write(&config_path, "{}").unwrap();

        {
            let mut root = state.workspace_root.write().unwrap();
            *root = Some(temp.path().to_path_buf());
        }

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&config_path).unwrap(),
                typ: FileChangeType::CHANGED,
            }],
        };

        // Initially no linter
        assert!(state.linter.read().unwrap().is_none());

        handle_did_change_watched_files(&state, params).await;

        // After reload_config, state.linter should be Some if it succeeded.
        // Even with empty config, it should be Some(Linter)
        assert!(state.linter.read().unwrap().is_some());
    }

    #[tokio::test]
    async fn test_handle_did_change_watched_files_no_config_change() {
        let state = BackendState::new();
        let temp = tempdir().unwrap();
        let config_path = temp.path().join(".tsuzulint.json");
        let other_path = temp.path().join("other.txt");
        fs::write(&config_path, "{}").unwrap();
        fs::write(&other_path, "hello").unwrap();

        {
            let mut root = state.workspace_root.write().unwrap();
            *root = Some(temp.path().to_path_buf());
        }

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&other_path).unwrap(),
                typ: FileChangeType::CHANGED,
            }],
        };

        // Initially no linter
        assert!(state.linter.read().unwrap().is_none());

        handle_did_change_watched_files(&state, params).await;

        // Should still be None
        assert!(state.linter.read().unwrap().is_none());
    }

    #[tokio::test]
    async fn test_handle_did_change_watched_files_mixed_changes() {
        let state = BackendState::new();
        let temp = tempdir().unwrap();
        let config_path = temp.path().join(".tsuzulint.json");
        let other_path = temp.path().join("other.txt");
        fs::write(&config_path, "{}").unwrap();
        fs::write(&other_path, "hello").unwrap();

        {
            let mut root = state.workspace_root.write().unwrap();
            *root = Some(temp.path().to_path_buf());
        }

        let params = DidChangeWatchedFilesParams {
            changes: vec![
                FileEvent {
                    uri: Url::from_file_path(&other_path).unwrap(),
                    typ: FileChangeType::CHANGED,
                },
                FileEvent {
                    uri: Url::from_file_path(&config_path).unwrap(),
                    typ: FileChangeType::CHANGED,
                },
            ],
        };

        handle_did_change_watched_files(&state, params).await;

        // Should be Some because one of them was a config file
        assert!(state.linter.read().unwrap().is_some());
    }
}
