//! LSP command implementation

use miette::Result;

use crate::utils::create_tokio_runtime;

pub fn run_lsp() -> Result<()> {
    create_tokio_runtime()?.block_on(async {
        tsuzulint_lsp::run().await;
    });
    Ok(())
}
