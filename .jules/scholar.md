## 2024-05-18 - Streamlining JSON serialization with serde
**Library:** serde v1.0 / serde_json v1.0
**Discovery:** Instead of allocating an intermediate `Vec<DiagnosticJson>` and relying on `serde_json::to_string(&items)`, we can wrap the slice `&[Diagnostic]` in a custom type and implement `serde::Serialize` utilizing `serializer.collect_seq()` to stream array elements without memory allocation for the intermediate `Vec`.
**Application:** `tzlint_wasm::diagnostics_to_json` was allocating `Vec<DiagnosticJson>` before turning into a JSON string. We can wrap the slice with a custom struct, and implement `serde::Serialize` directly to `serde_json::to_string` to avoid allocating `Vec<DiagnosticJson>`.
