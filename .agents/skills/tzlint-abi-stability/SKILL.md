---
name: tzlint-abi-stability
description: AstCoreV1 is permanently frozen; extend via additive tables; untrusted boundaries use checked rkyv::access.
---

**Rule.** `AstCoreV1` (`kind`, `span`, `parent`, `first_child`, `next_sibling`) is
**permanently frozen**. Endianness is pinned **little-endian**; `OptionNodeId` is a
`u32::MAX` sentinel newtype (not `Option<NodeId>`). New data (morphology, readings) goes
into **separate additive archived tables** with their own interface version — never new
fields on the frozen core. Untrusted boundary reads (plugin output, cache, cross-version)
use **checked** `rkyv::access` (`bytecheck`); `access_unchecked` only for host-written
data read back immediately (e.g. a plugin reading the host-written AST), with a comment.

**Why.** rkyv has no inherent schema compatibility — the archived layout is bound to the
type. A frozen core + additive tables + checked access is what makes adding data safe for
old rules and turns schema mismatch into `Err` instead of UB.

**How to apply.** Want to add AST data? Add a new table + interface version; do not modify
`AstCoreV1`. The `AstCoreV1` golden-bytes test must not change; if it does, you broke the
ABI. Confirm the encoding choice against the M0 spike before relying on it.
