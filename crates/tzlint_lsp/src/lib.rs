//! `tzlint_lsp` — the LSP server.
//!
//! v1 ships a scaffold only; the full server is milestone M5. When M5 is implemented it
//! will route through `tzlint_core::lint_document` — the same single dispatch entry used
//! by the CLI, cache, and fix — achieving CLI/LSP parity. The editing-latency strategy
//! (full reparse + debounce; block-level incremental cache, correctness-gated) and
//! `tree-sitter-md` as an incremental escape hatch are decided at M5.
//!
//! TODO(M5): implement the server.
