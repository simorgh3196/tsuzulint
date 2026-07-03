## 2025-05-18 - Optimize Serde Serialization
**Library:** serde v1.0, serde_json v1.0
**Discovery:** Avoid collecting iterators into intermediate `Vec` collections prior to JSON serialization. We can implement `serde::Serialize` on a custom wrapper struct and use `serializer.collect_seq()` to stream data directly without allocating intermediate DOM models or Vecs on the heap.
**Application:** Replaced `serde_json::json!` intermediate macro DOM building in `cache.rs` and `Vec::collect()` inside `diagnostics_to_json` in `tzlint_wasm/src/lib.rs` to stream serialization directly, avoiding heavy memory allocations and speeding up execution time, which is especially important for WebAssembly targets.
