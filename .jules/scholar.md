## 2025-01-01 - Serde stream serialization
**Library:** Serde v1.0
**Discovery:** `serializer.collect_seq()` allows streaming iterators directly into serializers without intermediate collection.
**Application:** Implemented a wrapper struct for diagnostics in `tzlint_wasm` to stream directly to JSON, bypassing an intermediate `Vec` heap allocation for better WebAssembly performance.
