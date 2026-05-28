# Rule development

> Status: template (M0/M1). Authoring rules with the Rust PDK.

- Implement `Rule`: `meta()` (id, `node_kinds`, required interfaces) + `check(node, cx)`;
  optional `finish(cx)` for cross-node rules (accumulate in `Context`, not `&self`).
- **Rule ids:** lowercase kebab-case; core rules bare (`no-doubled-joshi`), plugin rules
  namespaced `<namespace>/<rule>`.
- **Diagnostics:** `Diagnostic { rule_id, severity, message, span, fixes }`; messages come
  from a per-locale catalog (a language-specific rule may ship a single locale).
- **Fixes:** `Fix { span, replacement }` (absolute spans); overlaps resolved
  deterministically; applied to a fixpoint (≤10 passes).
- Reserves a future TS/AssemblyScript chapter.
