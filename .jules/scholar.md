## 2025-01-20 - serde_json Serialization Optimization
**Library:** serde v1.0, serde_json v1.0
**Discovery:** Avoid collecting iterators into intermediate Vec collections prior to serialization in performance-critical paths (like WASM bindings). Wrapping the slice/iterator in a custom struct and implementing `serde::Serialize` utilizing `serializer.collect_seq()` or `serializer.serialize_seq()` streams data directly without intermediate heap allocations.
**Application:** Used to optimize WASM `diagnostics_to_json` serialization to reduce intermediate memory allocations.
