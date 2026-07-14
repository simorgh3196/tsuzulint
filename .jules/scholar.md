## 2025-05-19 - Avoid Vec allocation during Serde JSON serialization
**Library:** Serde v1.0.x
**Discovery:** Iterating and collecting into an intermediate `Vec` just to serialize a sequence allocates memory unnecessarily. Serde provides `Serializer::collect_seq()` to stream an iterator directly into the sequence serialization.
**Application:** Replaced `.collect::<Vec<_>>()` and `serde_json::to_string(&vec)` in `diagnostics_to_json` with a wrapper struct that implements `serde::Serialize` utilizing `serializer.collect_seq()`. This avoids a heap allocation on the hot path of formatting diagnostics to JSON (especially important in `wasm32` constraints).
