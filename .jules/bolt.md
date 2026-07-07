## 2026-06-30 - Serialization Allocations in WASM
**Learning:** In WebAssembly performance, building intermediate Vecs for JSON serialization allocates on the heap unnecessarily.
**Action:** Wrap slices in structs and use serde's `collect_seq` instead of collecting maps into a Vec before passing to `serde_json::to_string()`.
