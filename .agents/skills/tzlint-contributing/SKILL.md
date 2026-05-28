---
name: tzlint-contributing
description: How to contribute — TDD, worktrees, confirm-before-push, just check, PR conventions, migrate-and-refactor + parity gate.
---

**Rule.**
- **TDD** (red-green-refactor) for rules, parser, config, io, the position mapper.
- Use **git worktrees** for isolated feature work.
- Run **`just check`** (rustfmt + clippy `-D warnings` + tests) before opening a PR.
- **Confirm before pushing**; do not push unreviewed/unverified work.
- **Conventional Commits** for messages; keep PRs focused.
- **Migrate-and-refactor + parity gate:** conforming legacy code is migrated/refactored
  (well-tested modules lifted intact with their tests); only non-conforming parts are
  reimplemented. The new core must reproduce the legacy CLI's diagnostics on the golden
  corpus (multiset of `(rule_id, severity, span)` matches) before the legacy repo is
  retired.

**Why.** These keep the rewrite shippable at every step and prevent reintroducing the
defect classes the redesign exists to eliminate.

**How to apply.** Start from a failing test; keep changes scoped; gate on `just check`;
ask before any externally-visible or destructive operation.
