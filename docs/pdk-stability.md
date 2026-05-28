# PDK stability policy

> Status: template (M0). What the core may change without breaking rules.

- `AstCoreV1` is **permanently frozen**; only additive tables are added.
- A new table / interface is a **minor** addition (existing rules unaffected).
- A change to a frozen core would be a **major** bump — avoided by design; governed here.
- Boundaries are `bytecheck`-validated so a version mismatch is an `Err`, not UB.
- Deprecation policy and the criteria for any major bump: TODO.
