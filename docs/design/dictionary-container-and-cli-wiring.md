# Dictionary container & CLI morphology wiring (M2j-wiring)

This is the slice that finally makes the morphology-dependent rules (today,
[`no-doubled-joshi`](../../crates/tzlint_rules/src/rules/no_doubled_joshi.rs)) **fire on real
Japanese text**. The frozen `MorphologyV1` model, the provider trait, the registry/fingerprint, the
analysis pass, the dictionary provisioning (verify → decompress → cache), and the native
[lindera](https://github.com/lindera-morphology/lindera) backend all already existed; what was
missing was the connection between *a provisioned dictionary blob* and *a working tokenizer*, and
between *that tokenizer* and *the CLI*.

## The problem

`provision_dictionary` returns the dictionary as a single in-memory `Vec<u8>` — the one
representation every host can produce, including a browser/wasm embedder with no filesystem. lindera,
however, builds a `Dictionary` from **several component files** (a prefix-dictionary trie, a
connection-cost matrix, character/unknown-word tables, and JSON metadata). lindera's public
filesystem loader wants a *directory* of those files; neither a directory nor a tar is something a
wasm host can hand it.

Crucially, lindera 3.0.7 also exposes an **in-memory** path: the `Dictionary` struct's fields are
public and each component has a public `load(bytes)` constructor (this is exactly how lindera's own
`embedded://` dictionaries are assembled from `include_bytes!` blobs). So no filesystem, no temp
directory, and **no tar dependency** are required — only a way to carry the component byte arrays
inside the single provisioned blob and split them back apart in memory.

## The container

`tzlint_core::dict::container` defines a tiny, positional, length-prefixed container that bundles the
eight component byte arrays into one blob — the *content* of the decompressed `.dict.zst`:

```text
offset  size  field
0       8     magic = b"TZDICTC1"      (trailing digit = format version; a bump is a new magic)
8       2     version: u16 = 1
10      2     member_count: u16 = 8
12      64    member table: 8 × { offset: u32, len: u32 }   (little-endian)
76      …     payload: the 8 member byte ranges
```

Members are **positional** (index → role; see `Member`), so there are no embedded name strings to
decode — one less untrusted-input surface. The module is deliberately **backend-agnostic**: it names
no lindera type and touches neither `Host` nor the network, so it compiles for `wasm32` and a future
browser backend reuses the exact same split.

`parse` is **panic-free on any byte string**: every field is read through bounds-checked slicing,
every member range is validated with checked arithmetic against the blob length, and members are
returned as borrowed slices, so a hostile length never drives an allocation. (The pin already
authenticates the compressed artifact and `provision_dictionary` caps the decompressed size at
`MAX_DICT`; the codec's own panic-safety is defense-in-depth for any future unpinned path.) Member
*content* validity — a malformed trie or archive — is the backend loader's contract, not the codec's.

## The bridge

`LinderaProvider::from_dictionary_bytes(&[u8])` (in the native-only `tzlint_morphology_native` crate)
parses the container and assembles a lindera `Dictionary` from the eight members via the five public
component loaders, mirroring lindera's embedded loader exactly (component order, `is_system = true`
for a system dictionary). It then converges on the same `Segmenter`/`Tokenizer` tail as the
directory and embedded constructors, so `analyze` is byte-for-byte identical no matter how the
dictionary was obtained.

One loader needs guarding: `ConnectionCostMatrix::load` reads `i16`s with `byteorder::read_i16_into`,
which **panics** on an odd-length matrix (and its old-format branch indexes by attacker-declared
sizes). The bridge therefore validates the matrix member up front — it must be the even-length
"new" format (leading `i16 == -1`) — and rejects anything else as a `MorphologyError::Backend`
rather than risk an abort. All other loaders (daachorse `deserialize`, rkyv checked `from_bytes`,
`serde_json`) already return `Result`, so every failure surfaces as `Backend`, never a panic.

### Packaging

`extract_components` (behind the dev-only `package` feature) is the inverse: it splits a *loaded*
`Dictionary` back into the eight component byte arrays — four verbatim, the two rkyv archives and the
metadata re-serialized value-exact, and the connection matrix re-encoded into the new `matrix.mtx`
format (the one component lindera keeps only as a parsed grid). The `pack_ipadic` example uses it to
build a `.dict` container from lindera's embedded IPADIC; compress it (`zstd`) and hash it (`b3sum`)
out of band to produce the distributable `.dict.zst` and its pin. The `embed-ipadic` round-trip test
packs the embedded IPADIC, rehydrates a provider through the bridge, and asserts its token stream is
**byte-identical** to the embedded reference — proving the round-trip (including the lossy matrix
re-encode and the `is_system` bit) is value-exact.

> **Build coupling.** `char_def.bin`/`unk.bin` are rkyv-0.8 archives and `dict.da` is a
> daachorse-2.1.1 blob, both tied to the exact `lindera-dictionary` 3.0.7 build. A container is only
> loadable by the same build; **re-pack and re-pin on any lindera bump.** The `embed-ipadic`
> round-trip test rebuilds the container in-process, so it trips the moment the pin must move.

## CLI wiring

A new, additive `morphology` config block names the source and pin:

```jsonc
{
  "morphology": {
    "path": "ipadic.dict.zst",   // OR "url": "https://…/ipadic.dict.zst"
    "pin": "<64 hex chars>",      // BLAKE3 over the COMPRESSED container
    "lang": "ja"                  // optional; only "ja" today
  }
}
```

It is validated at parse time (exactly one of `path`/`url`, a 64-hex pin decoded to 32 bytes, a
supported language) and resolved into `Config::morphology`. Absent (the default) it is `None`, so the
document cache key stays byte-identical to a pre-morphology run.

`app::build_morphology_registry` runs once per `lint`/`fix` and returns `Some(registry)` only when
**both** a source is configured **and** an enabled rule requires Japanese morphology (so a configured
but unused dictionary is never provisioned). When both hold it provisions the compressed container
through the `Host` (cached, verified against the pin), decompresses it, bridges it into a
`LinderaProvider`, and inserts it into a `MorphologyRegistry` keyed by `DictId::from_pin(pin)` — the
same pin the cache file is addressed by, so a dictionary upgrade changes both the cache file and the
morphology fingerprint. The registry is threaded into the three `lint_cached` / `lint_document` /
`fix` call sites that previously passed `None`. A provisioning or load failure aborts the run loudly;
a misconfigured or unreachable dictionary is an operator problem, not a silently-skipped rule.

`tzlint_morphology_native` becomes a dependency of the native CLI here — the first time lindera is
linked into a shipped binary. It can never enter the `wasm32` graph (the CLI is native-only, the
backend crate is `#![cfg(not(target_arch = "wasm32"))]`, and the io-guard tripwire keeps `lindera`
out of every other manifest).

## Out of scope

User-facing dictionary docs, per-dictionary licenses, a hosted IPADIC artifact, a "lite" preset, and
the browser/IndexedDB provider are deliberately left to later slices (M2k, M2n). This slice delivers
the codec, the bridge, the packaging building blocks, and the CLI wiring — and proves, end to end,
that `no-doubled-joshi` fires on real Japanese.
