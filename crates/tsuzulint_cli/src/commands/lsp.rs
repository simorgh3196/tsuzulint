//! LSP command implementation

use miette::{IntoDiagnostic, Result};

pub fn run_lsp() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(async {
            tsuzulint_lsp::run().await;
        });
    Ok(())
}
