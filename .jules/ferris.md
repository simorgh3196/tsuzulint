## 2024-06-07 - [Arc<str> over String for immutable identifiers]
**Learning:** Using `Arc<str>` for types that are used extensively as identifiers (e.g. `RuleId`) provides significant performance improvements compared to a plain `String` since these are mostly read-only, avoiding many expensive deep clones.
**Action:** Use `Arc<str>` or `Arc<[T]>` for similar types when implementing or refactoring codebase identifiers and immutable types.
