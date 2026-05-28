//! `tzlint_ast` — frozen ABI types for TsuzuLint.
//!
//! Holds the permanently-frozen `AstCoreV1`: an index-based AST (`NodeId(u32)` indices
//! into a contiguous node vector) plus `Span` (absolute byte offsets into the owned
//! source text). `no_std`-friendly; compiles for native and `wasm32`.
//!
//! TODO(M1): introduce `Ast`, `Node`, `NodeKind`, `Span`, `NodeId`, `OptionNodeId`
//! (sentinel `u32::MAX` == none) with rkyv derives, endianness pinned to little-endian,
//! per `docs/abi-spec.md`.
