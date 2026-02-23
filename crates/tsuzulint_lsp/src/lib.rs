//! TsuzuLint LSP Server
//!
//! Language Server Protocol implementation for TsuzuLint.
//! Provides real-time linting in editors.

mod config;
mod conversion;
mod debounce;
mod handler;
mod state;

use std::sync::Arc;

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
static TEST_LINT_DELAY_MS: AtomicU64 = AtomicU64::new(0);

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{debug, error, info};

use tsuzulint_core::{Diagnostic as TsuzuLintDiagnostic, Linter, LinterConfig};

use conversion::to_lsp_diagnostic;
use debounce::spawn_debounced_validation;
use handler::{
    handle_code_action, handle_did_change, handle_did_change_watched_files, handle_did_close,
    handle_did_open, handle_did_save, handle_document_symbol, handle_initialize,
    handle_initialized, handle_shutdown,
};
use state::{BackendState, SharedState};

/// The LSP backend for TsuzuLint.
#[derive(Clone)]
pub struct Backend {
    /// LSP client for sending notifications.
    client: Client,
    /// Shared state
    state: SharedState,
}

impl Backend {
    /// Creates a new backend with the given client.
    pub fn new(client: Client) -> Self {
        let config = LinterConfig::new();
        let linter = match Linter::new(config) {
            Ok(l) => Some(l),
            Err(e) => {
                error!(
                    "Failed to initialize linter: {}. LSP will run without linting.",
                    e
                );
                None
            }
        };

        Self {
            client,
            state: Arc::new(BackendState::with_linter(linter)),
        }
    }

    /// Validates a document and publishes diagnostics.
    async fn validate_document(&self, uri: Url, text: String, version: Option<i32>) {
        debug!("Validating document: {}", uri);

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                debug!("Skipping validation for non-file URI: {}", uri);
                return;
            }
        };

        let diagnostics = self.lint_text(&text, &path).await;

        let lsp_diagnostics: Vec<Diagnostic> = diagnostics
            .into_iter()
            .filter_map(|d| to_lsp_diagnostic(&d, &text))
            .collect();

        self.client
            .publish_diagnostics(uri, lsp_diagnostics, version)
            .await;
    }

    /// Lints text and returns TsuzuLint diagnostics.
    async fn lint_text(&self, text: &str, path: &std::path::Path) -> Vec<TsuzuLintDiagnostic> {
        let state = self.state.clone();
        let text = text.to_string();
        let path = path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            {
                let delay_ms = TEST_LINT_DELAY_MS.load(Ordering::Relaxed);
                if delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }

            let linter_guard = match state.linter.read() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    error!("Linter lock poisoned: {}", poisoned);
                    return vec![];
                }
            };

            let linter = match linter_guard.as_ref() {
                Some(l) => l,
                None => {
                    debug!("Linter not available, skipping linting");
                    return vec![];
                }
            };

            match linter.lint_content(&text, &path) {
                Ok(diagnostics) => diagnostics,
                Err(e) => {
                    error!("Lint error: {}", e);
                    vec![]
                }
            }
        })
        .await
        .unwrap_or_else(|e| {
            error!("lint_text task failed: {}", e);
            vec![]
        })
    }

    /// Sets a simulated delay for lint_text (test only, global state).
    #[cfg(test)]
    pub fn set_global_test_delay(delay: std::time::Duration) {
        TEST_LINT_DELAY_MS.store(delay.as_millis() as u64, Ordering::Relaxed);
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        handle_initialize(&self.state, &self.client, params).await
    }

    async fn initialized(&self, _: InitializedParams) {
        handle_initialized(&self.client).await;
    }

    async fn shutdown(&self) -> Result<()> {
        handle_shutdown().await
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let (uri, text, version) = handle_did_open(&self.state, params);

        self.validate_document(uri, text, version).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some((uri, text, version)) = handle_did_change(&self.state, params) {
            let backend = self.clone();

            spawn_debounced_validation(
                self.state.clone(),
                uri.clone(),
                text.clone(),
                version,
                move |u, t, v| {
                    let backend = backend.clone();
                    tokio::spawn(async move {
                        backend.validate_document(u, t, v).await;
                    });
                },
            );
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let (uri, text_opt) = handle_did_save(params);

        if let Some(text) = text_opt {
            self.validate_document(uri, text, None).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        handle_did_change_watched_files(&self.state, params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = handle_did_close(&self.state, params);

        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let state = self.state.clone();
        let text = {
            let docs = match state.documents.read() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Documents lock poisoned: {}", e);
                    return Ok(None);
                }
            };
            match docs.get(&params.text_document.uri) {
                Some(data) => data.text.clone(),
                None => return Ok(None),
            }
        };
        let path = params.text_document.uri.to_file_path().ok();

        let diagnostics = self.lint_text(&text, path.as_ref().unwrap()).await;

        handle_code_action(&self.state, &diagnostics, params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        handle_document_symbol(&self.state, params).await
    }
}

/// Starts the LSP server.
pub async fn run() {
    info!("TsuzuLint LSP server starting...");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::const_new(());

    include!("../tests/common_mod.rs");

    struct DelayGuard;
    impl Drop for DelayGuard {
        fn drop(&mut self) {
            Backend::set_global_test_delay(std::time::Duration::from_millis(0));
        }
    }

    #[tokio::test]
    async fn test_delay_guard_resets_delay() {
        let _lock = TEST_MUTEX.lock().await;

        Backend::set_global_test_delay(std::time::Duration::from_millis(50));
        assert_eq!(TEST_LINT_DELAY_MS.load(Ordering::Relaxed), 50);
        {
            let _guard = DelayGuard;
        }
        assert_eq!(TEST_LINT_DELAY_MS.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_lint_text_does_not_block_runtime() {
        use std::time::{Duration, Instant};

        let _lock = TEST_MUTEX.lock().await;
        let _guard = DelayGuard;

        Backend::set_global_test_delay(Duration::from_millis(100));

        let (client_read, server_write) = tokio::io::duplex(4096);
        let (server_read, client_write) = tokio::io::duplex(4096);

        let (service, socket) = LspService::new(Backend::new);

        let _server_handle = tokio::spawn(async move {
            tower_lsp::Server::new(server_read, server_write, socket)
                .serve(service)
                .await;
        });

        let mut reader = tokio::io::BufReader::new(client_read);
        let mut writer = client_write;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            while let Some(msg) = recv_msg(&mut reader).await {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });

        let temp_dir = tempfile::tempdir().unwrap();
        let root_path = temp_dir.path();
        let root_uri = Url::from_file_path(root_path).unwrap();

        let init_req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{}","capabilities":{{}}}}}}"#,
            root_uri
        );
        send_msg(&mut writer, &init_req).await;
        let _resp = rx.recv().await.unwrap();

        let initialized_notif = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
        send_msg(&mut writer, initialized_notif).await;

        let start = Instant::now();

        for i in 0..3 {
            let file_path = temp_dir.path().join(format!("test{}.md", i));
            let file_uri = Url::from_file_path(file_path).unwrap();
            let did_open = format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"markdown","version":0,"text":"test content {}"}}}}}}"#,
                file_uri, i
            );
            send_msg(&mut writer, &did_open).await;
        }

        let mut diagnostics_count = 0;
        let timeout = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                msg_opt = rx.recv() => {
                    if let Some(msg) = msg_opt {
                        if msg.contains("publishDiagnostics") {
                            diagnostics_count += 1;
                            if diagnostics_count >= 3 {
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
                _ = &mut timeout => break,
            }
        }

        let elapsed = start.elapsed();

        assert_eq!(diagnostics_count, 3, "Expected 3 diagnostics responses");

        println!("Elapsed time for 3 parallel lints: {:?}", elapsed);

        assert!(
            elapsed < Duration::from_millis(300),
            "Runtime was blocked! Expected < 300ms, got {:?}",
            elapsed
        );
    }
}
