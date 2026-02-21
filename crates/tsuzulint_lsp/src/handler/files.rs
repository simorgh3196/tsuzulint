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
