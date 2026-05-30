//! `tzlint_abi` — the shared-memory plugin ABI (host + guest sides).
//!
//! Defines the rkyv transport, the interface directory, the calling convention
//! (`tzlint_meta`/`tzlint_alloc`/`tzlint_lint`), interface versioning, and the
//! `bytecheck` boundary rules: untrusted plugin **output** is read with checked
//! `rkyv::access`; the host-written AST read by a plugin uses `access_unchecked`.
//! The chosen encoding (rkyv vs `MsgPack`) is confirmed by the M0 spike before freezing.
//!
//! TODO(M3): implement host/guest sides on wasmtime per `docs/abi-spec.md`.
