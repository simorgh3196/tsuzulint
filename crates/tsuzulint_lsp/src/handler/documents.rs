//! Document lifecycle handlers (open, change, save, close).

use tower_lsp::lsp_types::*;
use tracing::{debug, error};

use crate::state::{DocumentData, SharedState};

/// Handles the `textDocument/didOpen` notification.
pub async fn handle_did_open(
    state: &SharedState,
    params: DidOpenTextDocumentParams,
) -> (Url, String, Option<i32>) {
    debug!("Document opened: {}", params.text_document.uri);

    {
        let mut docs = match state.documents.write() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Documents lock poisoned: {}", e);
                return (params.text_document.uri, String::new(), None);
            }
        };
        docs.insert(
            params.text_document.uri.clone(),
            DocumentData {
                text: params.text_document.text.clone(),
                version: params.text_document.version,
            },
        );
    }

    (
        params.text_document.uri,
        params.text_document.text,
        Some(params.text_document.version),
    )
}

/// Handles the `textDocument/didChange` notification.
///
/// Returns the URI, text, and version for debounced validation.
pub async fn handle_did_change(
    state: &SharedState,
    params: DidChangeTextDocumentParams,
) -> Option<(Url, String, i32)> {
    debug!("Document changed: {}", params.text_document.uri);

    let change = params.content_changes.into_iter().next()?;
    let uri = params.text_document.uri.clone();
    let version = params.text_document.version;
    let text = change.text;

    {
        let mut docs = match state.documents.write() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Documents lock poisoned: {}", e);
                return None;
            }
        };
        docs.insert(
            uri.clone(),
            DocumentData {
                text: text.clone(),
                version,
            },
        );
    }

    Some((uri, text, version))
}

/// Handles the `textDocument/didSave` notification.
pub async fn handle_did_save(params: DidSaveTextDocumentParams) -> (Url, Option<String>) {
    debug!("Document saved: {}", params.text_document.uri);
    (params.text_document.uri, params.text)
}

/// Handles the `textDocument/didClose` notification.
pub async fn handle_did_close(state: &SharedState, params: DidCloseTextDocumentParams) -> Url {
    debug!("Document closed: {}", params.text_document.uri);

    {
        let mut docs = match state.documents.write() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Documents lock poisoned: {}", e);
                return params.text_document.uri;
            }
        };
        docs.remove(&params.text_document.uri);
    }

    params.text_document.uri
}
