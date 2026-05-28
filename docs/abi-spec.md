# Plugin ABI specification

> Status: template (M0/M3). The frozen contract for rules and future language PDKs.

- **Frozen `AstCoreV1`:** `kind`, `span`, `parent`, `first_child`, `next_sibling`.
  Endianness pinned **little-endian**; `OptionNodeId` = `u32::MAX` sentinel. Golden-byte
  layout test gates changes.
- **Additive tables:** `MorphologyV1`, `ReadingsV1`, … each with an interface version
  (`requires_interfaces` semver). Adding a table never breaks existing rules.
- **Calling convention (TODO M3):** guest exports `tzlint_abi_version`, `tzlint_meta`,
  `tzlint_alloc`/`tzlint_free`, `tzlint_lint(dir_ptr,dir_len) -> (ptr,len)`. Interface
  directory = fixed-layout header (not rkyv) of `{name,ver,off,len}` entries.
- **Boundary safety:** untrusted plugin **output** → checked `rkyv::access`; host-written
  AST read by a plugin → `access_unchecked` (justified). Encoding (rkyv/MsgPack) confirmed
  by the M0 spike.
