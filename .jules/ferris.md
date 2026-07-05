## 2024-07-05 - Avoid intermediate Vec allocations during JSON serialization
**Learning:** Collecting mapped iterators into a `Vec` before calling `serde_json::to_string` causes unnecessary heap allocations, particularly detrimental in memory-constrained WebAssembly environments.
**Action:** Define a custom wrapper struct for slices or iterators and implement `serde::Serialize` utilizing `serializer.collect_seq()` to stream data directly without intermediate heap allocations.
