## 2024-05-30 - WASM Diagnostics JSON Serialization Optimization
**Learning:** Collecting iterators into intermediate `Vec` instances before JSON serialization introduces unnecessary heap allocations, particularly impactful in memory-constrained targets like WebAssembly.
**Action:** Instead, wrap the slice in a custom struct and implement `serde::Serialize` using `serializer.collect_seq()` to stream data directly without intermediate allocations.
