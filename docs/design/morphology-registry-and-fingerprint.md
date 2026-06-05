# Morphology provider registry + dictionary cache fingerprint (M2e + M2h)

Status: **accepted** (2026-06-05). Scope: the M2e/M2h slice of the
[M2 (Morphology) roadmap](https://github.com/simorgh3196/tsuzulint/issues/41).

## Summary

This slice adds a per-run **`MorphologyRegistry`** — the seam through which an embedder
injects morphology providers (Japanese first) — and folds a **dictionary fingerprint** of
the *active* dictionaries into the document cache key, so a dictionary upgrade invalidates
stale cached diagnostics.

It deliberately stops short of running providers over a document. Building a `MorphologyV1`
table from the registry and feeding it to `Engine::lint` is the **analysis pass**, deferred
to **M2l** (the `no-doubled-joshi` rule, the first consumer). The native backend (**M2j**)
and the dictionary config surface (**M2n**) are likewise out of scope. This slice is the
registry + the cache key wiring, and nothing that depends on an unbuilt backend.

## Motivation

`cache.rs` already reserves a `CacheKeyInput.morphology_fingerprint: &[u8]` slot and folds it
into `document_cache_key` (step 6: a `kind` byte + a length-prefixed field), but `lint_cached`
hardcodes `&[]`. `Engine::lint` already accepts `Option<&ArchivedMorphologyV1>` and skips
morphology-requiring rules on a *presence* gate, with language matching explicitly deferred to
"the M2h provider registry". This slice builds that registry and feeds the reserved slot — no
new key shape, just a real value where an empty placeholder sat.

The correctness driver: the document cache is a pure function of its key, so *every* input that
can change a diagnostic must be in the key. Once a dictionary can change a diagnostic (M2l), the
dictionary's identity must be in the key. Wiring the fingerprint now — while the active set is
always empty in practice — lets us pin the **byte-identical-when-inactive** property before any
real dictionary exists to perturb it.

## Non-goals (explicitly deferred)

- **Analysis pass (M2l):** running providers over the AST to produce a `MorphologyV1` table and
  passing it to `Engine::lint`. `Engine::lint` is **not touched**; its gate stays presence-only.
  The registry stores each provider but this slice reads only `provider.lang()`.
- **Native lindera backend (M2j):** the future producer of `insert(...)` calls.
- **Dictionary config surface (M2n):** parsing user config into `(provider, DictId)` entries.
- **Provisioning / network in the key path:** `dict::provision_dictionary*` is **not** called by
  `lint_cached`. No `Host`, no `cache_dir`, no URL source enters this slice — the key path stays
  `Host`-free and `wasm32`-clean. The pinned BLAKE3 hash (already the verified content identity in
  `dict.rs`, checked before decompression) *is* the dictionary identity, whether or not the blob is
  currently materialized on disk.

## Architecture

A new module **`tzlint_core::morphology`** owns the registry. It lives in `tzlint_core` (not the
`no_std` `tzlint_pdk`) because it references `crate::RegionRules` and hashes with `blake3`, both
`tzlint_core` concerns. The name `MorphologyRegistry` is deliberately distinct from
`crate::Registry`, which is the unrelated **processor** registry (format → parser).

```rust
/// A dictionary identity: the BLAKE3 pin over the compressed dictionary blob (the value
/// `dict::provision_dictionary*` verifies and hash-addresses).
pub struct DictId([u8; 32]);

/// One registered language: its provider (run by the deferred M2l analysis pass) and the
/// dictionary identity folded into the fingerprint.
struct MorphologyEntry {                         // private
    provider: Box<dyn tzlint_pdk::MorphologyProvider>,
    dict_id: DictId,
}

/// Per-run morphology providers, injected Host-style by the embedder.
pub struct MorphologyRegistry {
    /// Invariant: sorted by `Lang::as_u32()`, one entry per `Lang` (last insert wins).
    entries: Vec<(Lang, MorphologyEntry)>,
}
```

**Injection** is imperative, mirroring how the processor `Registry` is built: the embedder
constructs the registry before linting and `insert`s a provider plus its `DictId`. The map key is
read from `provider.lang()`, so a key/value mismatch is unrepresentable. `tzlint_core` ships **no**
default providers; an unconfigured run uses `MorphologyRegistry::new()` (empty) and is byte-for-byte
identical to the pre-M2 cache key. The `DictId` is a zero-cost handoff of the `pinned_hash` the host
already passes to `dict::provision_dictionary` — provisioning the bytes to disk stays a separate
host concern, not wired into `lint_cached`.

The **`MorphologyProvider` trait gains no method.** Dictionary identity rides the core-side
`MorphologyEntry`, so the "additive-only with a default" rule for the public trait never even
applies — and the cross-dictionary aliasing hazard of a zero-default `fn dictionary_fingerprint()`
(distinct dictionaries hashing equal) is structurally avoided.

## Public API

```rust
// tzlint_core::morphology  (new module, re-exported at the crate root)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DictId([u8; 32]);
impl DictId {
    pub const fn from_pin(pin: [u8; 32]) -> Self;
    pub const fn as_bytes(&self) -> &[u8; 32];
}

#[derive(Debug, Default)]
pub struct MorphologyRegistry { /* entries: Vec<(Lang, MorphologyEntry)> */ }
impl MorphologyRegistry {
    pub fn new() -> Self;
    /// Register `provider` (keyed by `provider.lang()`) with dictionary identity `dict_id`,
    /// replacing any existing entry for that language (last insert wins).
    pub fn insert(&mut self, provider: Box<dyn tzlint_pdk::MorphologyProvider>, dict_id: DictId);
    /// The cache-key fingerprint of the dictionaries active for `rules`: the intersection of the
    /// registered languages and the languages `rules` require. Empty (`Vec::new()`) when that
    /// intersection is empty, so an inactive run keeps a byte-identical cache key.
    pub fn fingerprint(&self, rules: &crate::RegionRules) -> alloc::vec::Vec<u8>;
}

// tzlint_core::processor  (additive accessor on the existing RegionRules)
impl RegionRules {
    /// The dictionary languages every enabled base+column rule requires (duplicates allowed;
    /// callers dedupe). Mirrors `rule_ids()`; `filter_map`s each rule's `required_lang()`.
    pub fn required_langs(&self) -> alloc::vec::Vec<tzlint_ast::morphology::Lang>;
}

// tzlint_core::cache  (one new trailing Option parameter)
pub fn lint_cached(
    cache: &mut DocumentCache,
    ext: Option<&str>,
    content: &str,
    config: &Config,
    registry: &crate::Registry,                       // unchanged: the PROCESSOR registry
    processor_cfg: &crate::ProcessorConfig,
    rules: &crate::RegionRules,
    morphology: Option<&crate::MorphologyRegistry>,   // NEW
) -> Result<Vec<Diagnostic>, CacheError>;
```

No `is_empty`/`len`/builder variants are added — YAGNI, no consumer. `MorphologyEntry` stays
private. The `provider` field is stored now (confirmed) so M2l can read the provider for a given
`Lang` without a second parallel map; only `lang()` is read this slice.

## Fingerprint recipe

`fingerprint(rules)` computes the **active set** = registered languages whose `Lang` is in
`rules.required_langs()` (provisioned ∩ needed), then:

1. Collect `(lang, &DictId)` for entries whose `Lang` is required; re-sort by `Lang::as_u32()`
   defensively (the `entries` invariant already yields that order).
2. **If the active set is empty, return `Vec::new()`.** This single early return is the whole
   empty-preservation guarantee.
3. `let mut h = blake3::Hasher::new_derive_key("tsuzulint morphology fingerprint v1");`
   — a distinct derive-key domain, mirroring `cache.rs`'s `KEY_CONTEXT`/`CONFIG_CONTEXT`. The
   `v1` suffix is the single source of truth for this encoding's identity (no separate version
   const).
4. `h.update(&MORPHOLOGY_INTERFACE_VERSION.to_le_bytes());` — the `tzlint_ast::morphology` u32
   const, so a `V1 → V2` interface bump invalidates active-document keys.
5. `h.update(&(active.len() as u64).to_le_bytes());` — a count prefix, so distinct active sets
   cannot collide by re-segmentation.
6. For each `(lang, dict_id)` in order: `h.update(&lang.as_u32().to_le_bytes());` then
   `put(&mut h, dict_id.as_bytes());`, where the local `put` length-prefixes (u64-LE) exactly as
   `cache.rs::put` does. (`Lang::as_u32()`, not a non-existent `Lang::to_le_bytes()`.)
7. `h.finalize().as_bytes().to_vec()` → 32 non-empty bytes.

**Empty = none.** An empty `Vec` reaches `CacheKeyInput.morphology_fingerprint` as `&[]`;
`document_cache_key` step 6 computes `kind = u8::from(false) = 0` then `put(&[])`
(u64-LE `0` + no bytes) — bit-identical to every pre-M2 key. This holds in all three
half-configured cases: no providers; providers only for unneeded languages; needed languages with
no provider.

**Interface-version asymmetry (accepted).** Folding `MORPHOLOGY_INTERFACE_VERSION` *inside* the
non-empty branch means a `V1 → V2` bump invalidates keys **only for morphology-active documents**.
This is correct: an inactive document has no morphology behavior to invalidate, and its key must
stay pre-M2. The recipe docstring states this explicitly so the asymmetry is not silently
inherited.

**Pin = compressed-blob identity (accepted).** The folded `DictId` is the BLAKE3 of the
*compressed* dictionary — the value `dict.rs` verifies and hash-addresses. Re-compressing an
otherwise-identical dictionary changes the pin and churns active keys (spurious recompute, never
unsafe). A decompressed-content identity would require running provisioning before keying, which we
reject here to keep the key path `Host`-free and `wasm32`-clean.

## Cache wiring

`document_cache_key` and `CacheKeyInput` are **unchanged**. Inside `lint_cached`, after deriving
`rule_versions`/`processor`, the hardcoded `morphology_fingerprint: &[]` becomes:

```rust
let morphology_fingerprint: Vec<u8> = match morphology {
    Some(reg) => reg.fingerprint(rules),
    None => Vec::new(),
};
let key = document_cache_key(&CacheKeyInput {
    content, processor, config,
    rule_versions: &rule_versions,
    morphology_fingerprint: &morphology_fingerprint, // was &[]
})?;
```

The new parameter `morphology: Option<&MorphologyRegistry>` is type-distinct from the existing
`registry: &crate::Registry` (processor), so the naming collision is only nominal. On a cache miss,
`lint_cached` still calls `crate::lint_document → Engine::lint(.., None, ..)` exactly as today: the
fingerprint affects **keying only**, never diagnostics. The single production caller
(`crates/tzlint_cli/src/app.rs:217`) appends `None`; the in-crate `lint_cached` test sites append
`None` (or `Some(..)` for the new tests).

## Frozen-ABI & wasm safety

- **Frozen types untouched.** `MorphologyV1` / `AstCoreV1` are not referenced; their golden-layout
  and `const` size/align asserts are unaffected. `Lang` is read-only via `as_u32()` (no `Ord`
  added). `MORPHOLOGY_INTERFACE_VERSION` is read, not changed. `KEY_SCHEMA_VERSION` stays `2`.
- **Trait: zero methods added** to `MorphologyProvider`.
- **`wasm32`-clean.** `morphology.rs` imports only `blake3` (already a workspace dep),
  `tzlint_ast::morphology::{Lang, MORPHOLOGY_INTERFACE_VERSION}`, `tzlint_pdk::MorphologyProvider`,
  `crate::RegionRules`, and `alloc`/`std`. Never `crate::io::Host`, `crate::dict`, `crate::net`, or
  `ruzstd`. No new crate dependency.
- **No `unwrap`/`expect`/`panic!`.** Only infallible `as u32`/`as u64` casts and
  `finalize().as_bytes().to_vec()`.

## Test plan (TDD)

1. **Empty-key preservation (load-bearing):** `MorphologyRegistry::new().fingerprint(&rules)` is
   `Vec::<u8>::new()` for any `rules`, including a rule declaring `with_morphology` but with no
   matching provider.
2. **Pre-M2 byte-identity (gates the PR):** `lint_cached(.., None)` *and*
   `lint_cached(.., Some(&MorphologyRegistry::new()))` produce a `CacheKey` equal to
   `document_cache_key` with `morphology_fingerprint: &[]` — full path (real `Registry::with_builtins`,
   real `RegionRules`), not just a unit `document_cache_key` comparison.
3. **`KEY_SCHEMA_VERSION` unchanged:** asserted still `2`.
4. **Non-empty changes the key:** `WhitespaceProvider::new(Lang::JA)` + `DictId::from_pin([0xAB;32])`
   + a rule `with_morphology(Lang::JA)` ⇒ a 32-byte fingerprint and a `CacheKey` ≠ the empty-registry
   key (mirrors `morphology_kind_byte_forward_compat`).
5. **Dictionary-version addressing:** same provider/rule, `[0xAB;32]` vs `[0xCD;32]` ⇒ different
   fingerprints ⇒ different keys.
6. **Active-set intersection (both directions):** provider for JA but only a KO-needing (or
   no-morphology) rule ⇒ empty; rule needs JA but only a KO provider registered ⇒ empty.
7. **Determinism + order-invariance:** insert JA-then-KO vs KO-then-JA (both required) ⇒ identical
   fingerprint; `fingerprint` re-sorts so it does not depend on `required_langs()` ordering.
8. **`required_langs` union + dedup:** a base rule needs JA, a column rule needs JA+KO ⇒ the result
   contains JA and KO; a plain rule contributes nothing.
9. **Last-wins insert:** two providers for the same `Lang` with different `DictId`s ⇒ only the second
   survives; the fingerprint reflects the second.
10. **Interface-version fold (differential):** folding `MORPHOLOGY_INTERFACE_VERSION = 1` vs a
    hypothetical `2` over the same active set yields different digests — proves the version is in the
    hash without re-deriving the production byte sequence tautologically.
11. **Structural tripwire:** over every built-in rule meta, `needs_morphology()` ⇒
    `required_lang().is_some()`, so the `filter_map(required_lang)` silent-drop fails loudly if a
    future language-agnostic morphology rule lands.
12. **Miss-then-hit with morphology:** a non-empty registry ⇒ the miss computes diagnostics via the
    no-table `Engine::lint(.., None, ..)` path and caches; the second call hits the same key —
    confirming the fingerprint is stable and the analysis pass is untouched (diagnostics unchanged by
    registry presence).

## Files touched

- `crates/tzlint_core/src/morphology.rs` **(new, ~200 LOC):** `DictId`, `MorphologyEntry`,
  `MorphologyRegistry`, local `put` helper, `fingerprint`, + tests 1, 4–7, 9, 10.
- `crates/tzlint_core/src/lib.rs`: `pub mod morphology;` + `pub use morphology::{MorphologyRegistry, DictId};`.
- `crates/tzlint_core/src/processor/mod.rs`: `RegionRules::required_langs()` (mirrors `rule_ids()`,
  walks `base` + `columns[*].rules`, `filter_map(|r| r.meta().required_lang())`) + test 8.
- `crates/tzlint_core/src/cache.rs`: add the `morphology` parameter to `lint_cached`; compute the
  fingerprint; replace `&[]`; update the in-crate `lint_cached` test call sites to pass `None`; add
  tests 2, 3, 11, 12.
- `crates/tzlint_cli/src/app.rs` (line 217): append `None` (a real production caller — `just check`
  breaks without it).
- `crates/tzlint_ast/src/morphology.rs` (the `MORPHOLOGY_INTERFACE_VERSION` doc comment): correct
  the claim that the interface version "is also folded into the document cache key" to "is folded
  into the morphology fingerprint when morphology is active".

## Confirmed decisions

- **`Option<&MorphologyRegistry>` trailing parameter** (one-token `None` per call site) over a
  required `&MorphologyRegistry`.
- **Store `MorphologyEntry.provider` now**, so M2l reads the provider keyed by exactly this `Lang`
  without a later `insert` API churn.
- **Interface-version asymmetry accepted** (invalidates only morphology-active keys).
- **Pin = compressed-blob identity accepted** (re-compression churns active keys; never unsafe).
