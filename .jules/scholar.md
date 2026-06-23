## 2025-05-28 - serde_json to_string vs Value Display
**Library:** serde_json v1.0.150
**Discovery:** Constructing an intermediate `serde_json::Value` and calling `.to_string()` on it invokes the `Display` trait which is less efficient and less idiomatic than passing the structure or Value to `serde_json::to_string()`. `serde_json::to_string` directly serializes the structure to a `String` without the formatting overhead of the `Display` trait, especially for nested JSON objects like `Value::Array`.
**Application:** Replaced `Value::Array(items).to_string()` and `document.to_string()` with `serde_json::to_string(&items)` and `serde_json::to_string(&document)` in `tzlint_wasm` and `tzlint_core` cache to improve serialization efficiency and adhere to idiomatic usage.
