//! Initialize and shutdown handlers.

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tracing::{error, info};

use crate::config::reload_config;
use crate::state::BackendState;

/// Handles the `initialize` LSP request.
pub async fn handle_initialize(
    state: &BackendState,
    _client: &tower_lsp::Client,
    params: InitializeParams,
) -> Result<InitializeResult> {
    info!("TsuzuLint LSP server initializing...");

    if let Some(path) = params.root_uri.and_then(|u| u.to_file_path().ok()) {
        match state.workspace_root.write() {
            Ok(mut root) => {
                *root = Some(path);
            }
            Err(e) => {
                error!("Workspace root lock poisoned: {}", e);
                return Ok(InitializeResult::default());
            }
        }

        reload_config(state);
    }

    Ok(InitializeResult {
        capabilities: ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                        include_text: Some(true),
                    })),
                    ..Default::default()
                },
            )),
            code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
                code_action_kinds: Some(vec![
                    CodeActionKind::QUICKFIX,
                    CodeActionKind::SOURCE_FIX_ALL,
                ]),
                resolve_provider: Some(false),
                work_done_progress_options: Default::default(),
            })),
            document_symbol_provider: Some(OneOf::Left(true)),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "tsuzulint-lsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    })
}

/// Handles the `initialized` LSP notification.
pub async fn handle_initialized(client: &tower_lsp::Client) {
    client
        .log_message(MessageType::INFO, "TsuzuLint LSP server initialized!")
        .await;
}

/// Handles the `shutdown` LSP request.
pub async fn handle_shutdown() -> Result<()> {
    info!("TsuzuLint LSP server shutting down...");
    Ok(())
}
