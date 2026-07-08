## 2026-06-30 - Optimize intermediate JSON allocations with serde_json
**Library:** serde_json v1.0
**Discovery:** In memory-constrained environments, building a dynamic `serde_json::Value` only to stringify it with `.to_string()` incurs unnecessary heap allocations. We can implement `serde::Serialize` to stream data directly without intermediate collections.
**Application:** Replaced the intermediate mapping of `Diagnostic` into a `Vec<DiagnosticJson>` with a custom struct `DiagnosticList` that implements `serde::Serialize`. It uses `serializer.collect_seq()` to directly stream the data without creating a new `Vec`, avoiding allocations.
