## 2025-02-09 - Serde zero-copy array/sequence serialization
**Library:** serde v1.0 / serde_json v1.0
**Discovery:** When serializing a collection into a JSON array, collecting items into an intermediate `Vec` introduces unnecessary heap allocation. We can avoid this allocation by wrapping the slice in a custom struct and implementing `serde::Serialize` utilizing `serializer.collect_seq()` or `serializer.serialize_seq()` to stream data directly without intermediate heap allocations.
**Application:** Used this technique in `tzlint_wasm::diagnostics_to_json` to serialize the list of `Diagnostic` into JSON without an intermediate `Vec<DiagnosticJson>`.
