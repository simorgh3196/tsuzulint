## 2026-06-30 - WASM Serialization Allocation Optimization
**Learning:** In WebAssembly targets, collecting iterators into intermediate `Vec` collections (e.g. `Vec<DiagnosticJson>`) before `serde_json` serialization introduces unnecessary heap allocation and memory overhead, which can be detrimental in memory-constrained environments.
**Action:** Always wrap slices or iterators in a custom struct and implement `serde::Serialize` using `serializer.collect_seq()` to stream data directly without intermediate heap allocations.
