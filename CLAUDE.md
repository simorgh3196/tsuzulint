# CLAUDE.md

See [`AGENTS.md`](AGENTS.md) for the full contributor/agent guide and the
[`.agents/skills/`](.agents/skills/) skills. Key points:

- Documentation (`docs/`, README, Rustdoc) is written in **English**.
- Library crates forbid `.unwrap()`/`.expect()`/`panic!` (clippy-denied in CI).
- All boundary I/O goes through `tzlint_core::io`; no raw `fs::read*`/network elsewhere.
- `AstCoreV1` is frozen; extend via additive tables; untrusted boundaries use checked
  `rkyv::access`.
- Run `just check` (rustfmt + clippy + tests) before pushing.
