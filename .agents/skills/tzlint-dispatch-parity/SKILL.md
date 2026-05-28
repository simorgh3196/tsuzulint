---
name: tzlint-dispatch-parity
description: There is one dispatch function; any CLI/LSP/native/plugin divergence must be covered by a parity test.
---

**Rule.** CLI, LSP, native rules, and plugins all lint through the single
`Engine::lint(ast, rules)` entry point. Maintaining two dispatch loops (e.g. `lint_file`
vs `lint_content`) is forbidden. Any place where behavior could diverge by entry
(file vs buffer vs JS-provided text) must be covered by a **parity test** asserting
identical diagnostics for identical input, and native-vs-plugin implementations of the
same rule must agree.

**Why.** Divergent dispatch paths silently drift, producing different results in the
editor than on the CLI — a class of bug that is invisible until a user hits it.

**How to apply.** Adding an entry point or a fast path? Route it through `Engine::lint`
and add/extend a parity test. The migration parity gate (new core vs legacy CLI on the
golden corpus) is the same principle applied to the rewrite.
