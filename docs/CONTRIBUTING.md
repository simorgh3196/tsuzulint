# Contributing

> Status: template (M0). See [`../AGENTS.md`](../AGENTS.md) and `.agents/skills/`.

- **TDD** (red-green-refactor). Run **`just check`** (rustfmt + clippy `-D warnings` +
  tests) before opening a PR.
- Use **git worktrees** for isolated feature work; **confirm before pushing**.
- **Conventional Commits**; keep PRs focused.
- Library code must not use `.unwrap()`/`.expect()`/`panic!` (clippy-denied).
- Documentation and Rustdoc are written in **English**.
- Follow the **migrate-and-refactor + parity-gate** workflow during the rewrite.
