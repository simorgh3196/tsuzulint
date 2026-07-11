## 2025-01-01 - Optimize Wasm serialization
**Learning:** Collecting iterators into an intermediate `Vec` before serialization wastes memory and time, especially in Wasm.
**Action:** Wrap slices in a custom struct and use `serializer.collect_seq()` to stream data.
