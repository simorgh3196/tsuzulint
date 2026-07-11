## 2025-05-15 - Zero-copy Sequence Serialization in Serde
**Library:** serde v1.0
**Discovery:** To avoid allocating intermediate collections (`Vec`) when serializing iterators/slices, `serde::Serializer` provides methods like `collect_seq()`. This allows a custom `Serialize` implementation to stream items directly to the underlying format.
**Application:** Replaced the intermediate `Vec<DiagnosticJson>` allocation in `crates/tzlint_wasm/src/lib.rs` by wrapping the slice in a custom struct and implementing `Serialize` with `collect_seq()`. This minimizes heap fragmentation and allocation overhead, critical for the WebAssembly target.
