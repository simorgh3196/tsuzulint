# Roadmap

Milestones are independently valuable and gated. Browser is a first-class **design**
constraint from day one (the core compiles to `wasm32`, CI builds it); the first
**implementation** is the lean native core.

- **M0 — Bootstrap & encoding spike** *(this commit)*: new repo, 7-crate workspace
  skeleton, CI (incl. `wasm` + `io-guard` + `msrv`), skills, docs templates, LICENSE.
  Spike: rkyv vs MsgPack vs FlatBuffers on the **real** wasmtime path; go/no-go = adopt
  rkyv only if ≥1.5× over MsgPack on real-path throughput, else MsgPack. Record before
  freezing the ABI. **DONE: rkyv wins ≈11.3× over MsgPack (≈4.6× over FlatBuffers) on the
  real path → GO (rkyv); encoding no longer provisional. See
  [`research/encoding-spike.md`](research/encoding-spike.md).**
- **M1 — Lean core**: `tzlint_ast` (frozen AST), `tzlint_core` (markdown-rs parser +
  mdast→index transform, multi-format config + presets, document-level cache,
  single-traversal engine, position mapper, `io` + `Host` abstraction), Diagnostic/Fix
  model + autofix, `tzlint_rules` starter set, `tzlint_cli` (`lint`/`fix`/`init`), full
  tests + docs. **Migration parity gate** asserted here.
- **M2 — Morphology**: language-neutral provider; Japanese first; `MorphologyV1` table;
  dynamic (non-embedded) dictionary provisioning; dictionary-version cache key.
- **M3 — Plugin ABI (native)**: `tzlint_abi` transport + calling convention on wasmtime
  (instance reuse/pooling); `tzlint_pdk`; ABI golden-byte tests; benchmarks.
- **M4 — Browser**: `web/` wasm bindings + playground; shared-memory ABI; COOP/COEP;
  AST-integrity posture decided.
- **M5 — LSP**: full LSP over the shared engine with CLI parity; editing-latency strategy
  (full reparse + debounce; block-level incremental cache, correctness-gated).
- **M6 — Registry**: migrate the SSRF/security module intact with its tests; two-layer
  trust model (IPv4+IPv6) + authenticated private-repo access.
