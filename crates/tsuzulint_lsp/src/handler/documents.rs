//! Document lifecycle handlers (open, change, save, close).

use tower_lsp::lsp_types::*;
use tracing::{debug, error};

use crate::state::{DocumentData, SharedState};

/// Handles the `textDocument/didOpen` notification.
pub fn handle_did_open(
    state: &SharedState,
    params: DidOpenTextDocumentParams,
) -> (Url, String, Option<i32>) {
    let uri = params.text_document.uri;
    let text = params.text_document.text;
    let version = params.text_document.version;

    debug!("Document opened: {}", uri);

    {
        let mut docs = match state.documents.write() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Documents lock poisoned: {}", e);
                return (uri, text, Some(version));
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

    (uri, text, Some(version))
}

/// Handles the `textDocument/didChange` notification.
///
/// Returns the URI and version for debounced validation.
pub fn handle_did_change(
    state: &SharedState,
    params: DidChangeTextDocumentParams,
) -> Option<(Url, i32)> {
    let uri = params.text_document.uri;
    let version = params.text_document.version;

    debug!("Document changed: {}", uri);

    let change = params.content_changes.into_iter().next()?;
    let text = change.text;

    {
        let mut docs = match state.documents.write() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Documents lock poisoned: {}", e);
                return None;
            }
        };
        docs.insert(uri.clone(), DocumentData { text, version });
    }

    Some((uri, version))
}

/// Handles the `textDocument/didSave` notification.
pub fn handle_did_save(params: DidSaveTextDocumentParams) -> (Url, Option<String>) {
    debug!("Document saved: {}", params.text_document.uri);
    (params.text_document.uri, params.text)
}

/// Handles the `textDocument/didClose` notification.
pub fn handle_did_close(state: &SharedState, params: DidCloseTextDocumentParams) -> Url {
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
