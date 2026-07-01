## 2025-07-01 - Avoid iterator to Vec collection before serialization
**Learning:** To optimize serialization performance (especially in memory-constrained targets like WebAssembly), avoid collecting iterators into intermediate `Vec` collections prior to serialization.
**Action:** Wrap the slice or iterator in a custom struct and implement `serde::Serialize` utilizing `serializer.collect_seq()` to stream data directly without intermediate allocations.
