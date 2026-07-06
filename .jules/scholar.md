## 2024-05-18 - Serialize Iterators Directly
**Library:** serde v1.0
**Discovery:** `serde::Serializer::collect_seq` allows serializing an iterator directly without collecting into an intermediate collection.
**Application:** Replaced the intermediate `Vec` allocation in `diagnostics_to_json` with a custom struct wrapping the slice and using `collect_seq` to stream the data, improving memory efficiency in the WASM target.
