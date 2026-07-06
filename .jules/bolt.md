## 2024-07-06 - Stream JSON serialization for WebAssembly
**Learning:** Serializing slices of data to JSON by first mapping to an intermediate `Vec<T>` allocates memory unnecessarily. This overhead is particularly detrimental in Wasm where allocations are expensive.
**Action:** Implement `serde::Serialize` on a wrapper struct and use `serializer.collect_seq()` to stream iterator values directly into the JSON serializer, bypassing the intermediate heap allocation.
