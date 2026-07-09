
## 2025-02-18 - Serialize JSON Directly without Intermediate Value Objects
**Learning:** Using `serde_json::json!` or building intermediate `serde_json::Value` structures significantly reduces serialization performance by introducing extensive heap allocations (HashMaps, Vecs, Strings).
**Action:** When serializing structs (especially in performance-sensitive contexts like cache-saving loops), define intermediate struct wrappers that implement `serde::Serialize` to stream serialization directly. This drastically reduces memory allocations and improves performance.
