## 2024-07-05 - Avoid intermediate collections prior to JSON serialization
**Learning:** In performance-critical paths like WASM bindings, intermediate `Vec` collections allocated strictly to feed into `serde_json::to_string` incur unneeded memory allocation and latency, especially in a constrained target.
**Action:** Instead of collecting into an intermediate `Vec`, wrap the slice in a custom struct and implement `serde::Serialize` calling `serializer.collect_seq()` to stream data directly without intermediate heap allocations.
