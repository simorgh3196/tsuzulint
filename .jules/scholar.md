## 2025-07-02 - Eliminate intermediate allocation during serialization
**Library:** serde v1.0
**Discovery:** Serde provides `serializer.collect_seq()` which streams elements from an iterator sequentially.
**Application:** Used a struct wrapping an iterator (slice of diagnostics) to serialize directly to JSON using `serializer.collect_seq`, which avoids allocating an intermediate vector that could be expensive, especially on WebAssembly targets where memory constraints apply.
