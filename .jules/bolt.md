## 2024-05-28 - WASM JS bindings JSON DOM allocation
**Learning:** Building `serde_json::Value` (JSON DOM) structures inside hot iterators (like WASM binding boundary `diagnostics_to_json`) triggers multiple allocations that can be completely avoided by defining a local struct deriving `serde::Serialize`.
**Action:** Always define structured types with `#[derive(Serialize)]` instead of `serde_json::json!` and `Value::Array` to stringify collections directly for WASM boundary interactions, avoiding dynamic allocations.
