//! Texide LSP Server
//!
//! Language Server Protocol implementation for Texide.
//! Provides real-time linting in editors.

use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    info!("Texide LSP server starting...");

    // TODO: Implement LSP server using tower-lsp or similar
    // This is a placeholder for the LSP implementation

    eprintln!("LSP server not yet implemented");
    eprintln!("For now, use the CLI: texide lint <files>");
}
