## 2024-07-10 - Serde JSON macro overhead
**Learning:** The `serde_json::json!` macro allocates an intermediate DOM (`serde_json::Value`) which introduces overhead for serialization in performance-critical paths, especially in WebAssembly targets.
**Action:** Use a local struct with `#[derive(serde::Serialize)]` and pass it directly to `serde_json::to_string()` (or `serde_json::to_vec()`) instead of using `serde_json::json!`.
