## 2024-05-19 - Serde Streaming Serialization
**Library:** serde v1.0.x
**Discovery:** Serde's `Serializer` trait provides `collect_seq` which allows streaming data into a sequence directly from an iterator. This avoids collecting intermediate vectors when serializing collections, leading to faster execution and reduced heap allocations, especially beneficial for memory-constrained targets like WebAssembly.
**Application:** Refactored `diagnostics_to_json` in `crates/tzlint_wasm/src/lib.rs` to use `serializer.collect_seq()` through a custom wrapper struct `DiagnosticsWrapper`, eliminating the intermediate `Vec<DiagnosticJson>` allocation before serialization.
