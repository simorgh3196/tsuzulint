## 2025-07-07 - Avoid Intermediate Vec Allocations in Serde
**Library:** serde v1.0
**Discovery:** Serde serializers provide a `collect_seq` method (and related `serialize_seq`) that accepts an iterator. This allows streaming data directly to the serializer without needing to collect elements into an intermediate `Vec`.
**Application:** Used a wrapper struct `DiagnosticList` with a custom `Serialize` implementation using `serializer.collect_seq()` in the WebAssembly boundary to serialize diagnostics directly to JSON, eliminating a `Vec` heap allocation in a performance-critical path.
