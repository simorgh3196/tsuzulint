# Agent guide (TsuzuLint)

Repository-specific guidance for AI-assisted and human contributors. Skills live under
[`.agents/skills/`](.agents/skills/) and encode the "do / don't" rules; read the relevant
one before changing that area.

- **`tzlint-io-safety`** — all boundary I/O goes through `tzlint_core::io`; no raw
  `fs::read*`/network elsewhere; atomic writes; TOCTOU avoidance.
- **`tzlint-abi-stability`** — `AstCoreV1` is frozen (little-endian, `OptionNodeId`
  sentinel); extend via new tables; untrusted boundaries use checked `rkyv::access`.
- **`tzlint-performance`** — never JSON-serialize / per-node-deserialize the AST; spans are
  absolute into `Ast.text`; single-traversal is native-rule-only; plugins get one hand-off
  per file; share the compiled WASM module via `Arc` and reuse instances per worker.
- **`tzlint-dispatch-parity`** — one dispatch function; CLI/LSP/native/plugin divergence
  needs a parity test.
- **`tzlint-security-net`** — two-layer SSRF (IPv4 + IPv6); token = scheme+host+port with
  redirect-stripping; validate the dialed IP.
- **`tzlint-contributing`** — TDD; `just check` before pushing; confirm-before-push; PR
  conventions; the migrate-and-refactor + parity-gate workflow.

**Conventions:** Documentation under `docs/` and Rustdoc are written in **English**.
Library code must not use `.unwrap()`/`.expect()`/`panic!` (clippy-denied). Run
`just check` before opening a PR.
