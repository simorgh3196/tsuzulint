//! Debouncing utilities for LSP notifications.

use std::time::Duration;

use tower_lsp::lsp_types::Url;
use tracing::error;

use crate::state::{BackendState, SharedState};

/// Default debounce delay in milliseconds.
pub const DEFAULT_DEBOUNCE_MS: u64 = 300;

/// Spawns a debounced validation task.
///
/// This function waits for the debounce period, then checks if the document
/// version is still the same before retrieving the text and triggering validation.
pub fn spawn_debounced_validation<F>(
    state: SharedState,
    uri: Url,
    version: i32,
    validate_fn: F,
) where
    F: FnOnce(Url, String, Option<i32>) + Send + 'static,
{
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(DEFAULT_DEBOUNCE_MS)).await;

        if let Some(text) = get_text_if_version(&state, &uri, version) {
            validate_fn(uri, text, Some(version));
        }
    });
}

/// Retrieves the document text if the version matches.
fn get_text_if_version(state: &BackendState, uri: &Url, version: i32) -> Option<String> {
    let docs = match state.documents.read() {
        Ok(g) => g,
        Err(e) => {
            error!("Documents lock poisoned: {}", e);
            return None;
        }
    };

    if let Some(doc) = docs.get(uri) {
        if doc.version == version {
            return Some(doc.text.clone());
        }
    }
    None
}
