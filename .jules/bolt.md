## 2026-07-13 - Wasm diagnostics serialization intermediate allocation
**Learning:** In the `tzlint_wasm` crate, building an intermediate `Vec<DiagnosticJson>` in `diagnostics_to_json` adds heap overhead that can be bypassed using `serializer.collect_seq()` through a custom wrapper struct implementing `serde::Serialize`.
**Action:** Use streaming serialization and wrapper types rather than intermediate vectors to keep allocations minimal in WASM context and performance critical code.
