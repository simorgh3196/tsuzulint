## 2025-02-28 - Serialize Iterator directly
**Library:** serde_json v1.0.150
**Discovery:** In memory-constrained environments or to reduce allocations, you can implement `serde::Serialize` to serialize data without intermediate `Vec` collections. Use `serializer.collect_seq()` to stream iterator elements.
**Application:** Used in `crates/tzlint_wasm/src/lib.rs` to stream the diagnostics to JSON, avoiding a `Vec<DiagnosticJson>` allocation.
