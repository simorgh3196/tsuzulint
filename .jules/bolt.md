## 2025-05-28 - Avoid Intermediate Vec Allocations in Serde Serialization
**Learning:** Collecting iterators into intermediate `Vec` collections prior to serialization incurs unnecessary heap allocations. This overhead is particularly detrimental in memory-constrained environments like WebAssembly.
**Action:** Implement `serde::Serialize` manually on wrapper structs and use `serializer.collect_seq()` to stream iterators directly into the serializer, preventing the intermediate allocation altogether.
