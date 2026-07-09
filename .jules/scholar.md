## 2026-07-09 - Zero-copy sequence serialization in memory-constrained targets
**Library:** serde v1.0
**Discovery:** Iterators can be serialized directly without allocating intermediate memory via `serializer.collect_seq()` or `serializer.serialize_seq()`.
**Application:** Avoids unnecessary intermediate heap collections into `Vec` prior to serialization, crucial for WebAssembly where avoiding heap allocations significantly improves performance.
