use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};
use tracing_subscriber::prelude::*;
use tsuzulint_lsp::Backend;

struct LogCounter(Arc<Mutex<usize>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogCounter {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        if visitor.0.contains("Validating document") {
            *self.0.lock().unwrap() += 1;
        }
    }
}

struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let _ = write!(self.0, "{:?}", value);
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

#[tokio::test]
async fn test_did_change_validation_frequency() {
    let counter = Arc::new(Mutex::new(0));
    let log_layer = LogCounter(counter.clone());
    let subscriber = tracing_subscriber::registry().with(log_layer);

    // Try to set global default. If it fails, we might miss logs.
    let _ = tracing::subscriber::set_global_default(subscriber);

    let (service, _) = LspService::new(Backend::new);
    let uri = Url::parse("file:///tmp/test.md").unwrap();

    // Initialize
    let _ = service
        .inner()
        .initialize(InitializeParams::default())
        .await;
    service.inner().initialized(InitializedParams {}).await;

    // Send 5 rapid changes
    for i in 1..=5 {
        service
            .inner()
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: i,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: format!("Change {}", i),
                }],
            })
            .await;

        // Small delay
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Wait a bit at the end
    tokio::time::sleep(Duration::from_millis(500)).await;

    let count = *counter.lock().unwrap();
    println!("Total validations: {}", count);

    // With debouncing, we expect significantly fewer validations (ideally 1).
    assert!(
        count <= 2,
        "Expected debouncing to reduce validations (actual: {})",
        count
    );
    assert!(
        count >= 1,
        "Expected at least 1 validation (actual: {})",
        count
    );
}
