## 2025-02-15 - Zero-copy Serialize for Iterators
**Library:** serde v1.0.228
**Discovery:** To optimize serialization performance (especially in memory-constrained targets like WebAssembly), avoid collecting iterators into intermediate `Vec` collections prior to serialization. Instead, wrap the slice or iterator in a custom struct and implement `serde::Serialize` utilizing `serializer.collect_seq()` or `serializer.serialize_seq()` to stream data directly without intermediate heap allocations.
**Application:** Avoided an intermediate allocation when serializing diagnostics by wrapping `&[Diagnostic]` into a struct `DiagnosticsWrapper` implementing `serde::Serialize` with `serializer.collect_seq()`.
