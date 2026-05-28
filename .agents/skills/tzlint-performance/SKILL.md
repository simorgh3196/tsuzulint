---
name: tzlint-performance
description: Hot-path performance rules — no JSON/per-node deserialize, absolute spans, native-only single traversal, instance reuse.
---

**Rule.**
- Never JSON-serialize the AST, and never `Deserialize` the archive to native nodes
  per-node in the hot path — rules read the archived form in place via `NodeRef<'ast>`.
- `Span` is an **absolute** byte range into `Ast.text`; never pass `source` as a separate
  payload; no node-relative rebasing.
- **Single-traversal is a native-rule property only.** Plugins receive the whole archive
  **once per (file, plugin)** and self-traverse; never call a plugin per node.
- Config membership lookups use `HashSet`; precompute outside the file loop.
- Share the compiled WASM `Module` via `Arc`; reuse `Store`/`Instance` per rayon worker
  (reset between files); the host-written AST is read with `access_unchecked` (no O(N)
  re-validation per plugin).

**Why.** The original design's JSON serialization in the hot path negated the AST's cache
locality; per-node boundary crossings and per-plugin revalidation are the other cliffs.

**How to apply.** If a change adds an allocation/copy/serialize per node, or a host↔guest
crossing per node, stop — it is almost certainly wrong. Benchmark the real WASM path.
