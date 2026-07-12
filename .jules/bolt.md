## 2026-07-12 - Streaming Serialization for Diagnostics JSON
**Learning:** In memory-constrained targets like WebAssembly, collecting iterators into intermediate `Vec` collections prior to serialization creates unnecessary heap allocations and slows down serialization.
**Action:** Instead of `collect()`ing into a `Vec`, wrap the slice or iterator in a custom struct and implement `serde::Serialize` utilizing `serializer.collect_seq()` to stream data directly without intermediate heap allocations.
