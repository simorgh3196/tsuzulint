## 2025-05-18 - Serialize with `serializer.collect_seq()` to avoid allocating intermediate vectors
**Library:** serde v1.0.228
**Discovery:** Instead of allocating an intermediate `Vec` or iterating and using `.collect::<Vec<_>>()`, `serde::Serializer` provides `collect_seq` which streams elements into the sequence directly.
**Application:** Used in `crates/tzlint_wasm/src/lib.rs` to stream `Diagnostic` to `DiagnosticJson` avoiding `Vec` allocations, which improves performance and avoids memory pressure in WASM.
