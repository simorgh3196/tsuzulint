## 2025-05-28 - serde_json to_string optimization
**Library:** serde_json v1.0
**Discovery:** Constructing intermediate JSON DOMs via `serde_json::json!` or `Value::Array` before string serialization incurs unnecessary heap allocations. Using a local `#[derive(Serialize)]` struct and passing it to `serde_json::to_string` is significantly faster.
**Application:** Replaced the intermediate array allocations in `tzlint_wasm::diagnostics_to_json` with a local `JsonDiagnostic` struct.
