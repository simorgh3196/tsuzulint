## 2023-10-27 - Zero-Copy JSON Array Serialization in Wasm
**Learning:** Collecting iterators into intermediate `Vec` allocations (e.g. `Vec<DiagnosticJson>`) just to pass to `serde_json::to_string` causes unnecessary memory overhead, particularly impactful in memory-constrained targets like WebAssembly where allocations are slower.
**Action:** Instead of collecting into an intermediate `Vec`, wrap the slice/iterator in a proxy struct and implement `serde::Serialize` on it directly, utilizing `serializer.serialize_seq()` to stream elements dynamically without intermediate heap allocations.
