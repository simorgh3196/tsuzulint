# Plugin ABI specification

> Status: `AstCoreV1` frozen in M1a (`tzlint_ast`). Calling convention is still M3.
> The frozen contract for rules and future language PDKs.

- **Frozen `AstCoreV1` (`tzlint_ast`):** each `Node` is the record
  `{ kind, span, parent, first_child, next_sibling }`, archived to a **24-byte**,
  padding-free layout: `kind: u32`, `span: { start: u32, end: u32 }`, `parent: u32`,
  `first_child: u32`, `next_sibling: u32`. Pinned **little-endian** with **32-bit relative
  pointers** (rkyv features `little_endian` + `aligned` + `pointer_width_32`), so one
  archive is byte-identical across hosts and read in place on native (64-bit) and `wasm32`
  (32-bit) alike. `OptionNodeId` is a bare `u32` with `u32::MAX` as the `None` sentinel
  (not `Option<NodeId>`, which would add a discriminant). The `Ast` wire type derives only
  `Archive`/`Serialize` — **no `Deserialize`** — so consumers read `ArchivedAst` in place.
- **`kind` is a forward-compatible open enum (`NodeKind`):** the wire value is a bare
  `u32`, so any value round-trips and checked `rkyv::access` validates a plain integer, not
  a closed variant set. The 34 known kinds (discriminants `0..=33`) map 1:1 onto the
  markdown-rs `mdast::Node` vocabulary; an unknown kind from a newer producer is
  **preserved and treated as opaque** by an older consumer rather than rejected.
  Discriminants are frozen: a new kind appends the next free value and never moves or
  reuses an existing one.
- **Golden-byte lock:** `tzlint_ast`'s `golden_archived_layout_is_frozen` test pins the
  exact archived image of a canonical tree, and `const`-asserts pin the record sizes. Any
  drift — field reorder, endianness/pointer-width change, or an rkyv upgrade that alters
  the layout — fails CI. Re-baselining is a deliberate, major-version act.
- **Additive tables:** `MorphologyV1`, `ReadingsV1`, … keyed by `NodeId` (never by
  extending `Node`), each with an interface version (`requires_interfaces` semver). Adding
  a table never breaks existing rules.
- **Calling convention (TODO M3):** guest exports `tzlint_abi_version`, `tzlint_meta`,
  `tzlint_alloc`/`tzlint_free`, `tzlint_lint(dir_ptr,dir_len) -> (ptr,len)`. Interface
  directory = fixed-layout header (not rkyv) of `{name,ver,off,len}` entries.
- **Boundary safety:** untrusted plugin **output** → checked `rkyv::access`; host-written
  AST read by a plugin → `access_unchecked` (justified). Encoding (rkyv/MsgPack) confirmed
  by the M0 spike.
