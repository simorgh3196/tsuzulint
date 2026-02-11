## 2024-05-24 - Zero-Copy AST Serialization
**Learning:** `serde_json::value::RawValue` is a powerful tool for partial serialization/deserialization. In a plugin system where a large AST is passed to multiple plugins, serializing the AST to a `RawValue` once and passing that reference prevents redundant serialization for every plugin call.
**Action:** When designing plugin architectures with JSON-based interop, identify immutable context data (like ASTs) and use `RawValue` or pre-serialized bytes to avoid O(N) serialization cost where N is the number of plugins.
