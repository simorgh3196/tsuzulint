//! `tzlint_lsp` — the LSP server.
//!
//! Goes through the same `tzlint_core::Engine::lint` as the CLI (parity asserted in
//! tests). v1 ships a scaffold only; the full server is milestone M5, where a stated
//! editing-latency strategy (full reparse + debounce; block-level incremental cache,
//! correctness-gated) and `tree-sitter-md` as an incremental escape hatch are decided.
//!
//! TODO(M5): implement the server.
