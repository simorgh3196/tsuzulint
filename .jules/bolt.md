# Bolt

## 2024-05-24 - Zero-Copy AST Serialization

**Learning:** `serde_json::value::RawValue` is a powerful tool for partial serialization/deserialization. In a plugin system where a large AST is passed to multiple plugins, serializing the AST to a `RawValue` once and passing that reference prevents redundant serialization for every plugin call.
**Action:** When designing plugin architectures with JSON-based interop, identify immutable context data (like ASTs) and use `RawValue` or pre-serialized bytes to avoid O(N) serialization cost where N is the number of plugins.

## 2025-05-27 - Zero-Copy AST Child Construction

**Learning:** Recursively building ASTs by collecting children into a `Vec` and then cloning into an arena (`alloc_slice_clone`) is a performance anti-pattern. Using `bumpalo::Bump::alloc_slice_fill_iter` allows constructing the slice directly in the arena, avoiding the intermediate allocation and copy.
**Action:** When working with `bumpalo` (or `AstArena`), always prefer `alloc_slice_fill_iter` over `collect::<Vec<_>>` + `alloc_slice_clone` for dynamic sequences.
