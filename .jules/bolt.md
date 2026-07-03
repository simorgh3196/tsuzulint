## 2024-05-24 - Streaming Serialization in WebAssembly
**Learning:** Collecting iterators into intermediate `Vec` collections prior to serialization introduces unnecessary heap allocations, which is especially detrimental in memory-constrained targets like WebAssembly.
**Action:** Wrap the slice or iterator in a custom struct and implement `serde::Serialize` utilizing `serializer.collect_seq()` or `serializer.serialize_seq()` to stream data directly without intermediate heap allocations.
