# Encoding spike — rkyv vs MsgPack vs FlatBuffers

> Technology-selection investigation (internal). This research gated the plugin-transport
> choice. For user-facing performance vs other linters, see
> [`../benchmarks.md`](../benchmarks.md).

**Decision: GO — adopt `rkyv`** for the plugin transport. The encoding is no longer
provisional.

## Environment & versions

Absolute numbers are **machine-, version-, and time-dependent** and are meaningless without
the conditions below; the decision rests on the *ratios*, not the absolutes. Re-measure (and
re-record this table) on any toolchain/hardware change.

| | |
| --- | --- |
| Date measured | 2026-05-29 (JST) |
| Hardware | Apple M1 Pro · 10 cores · 32 GB |
| OS | macOS 26.2 (Darwin 25.2.0, arm64) |
| Toolchain | rustc / cargo 1.96.0, `--release` |
| Crates | rkyv 0.8.16 · rmp-serde 1.3.1 · flatbuffers 25.12.19 |
| WASM host runtime | wasmtime 45.0.0 |
| FlatBuffers compiler | flatc 25.12.19 |
| Guest target | wasm32-unknown-unknown |

## Method

A representative index-based AST of **100,000 nodes** over a ~3.49 MB CJK+ASCII
markdown-like text (`Vec<Node>` + an owned `String`, the frozen `AstCoreV1` shape) was
encoded with each format and exercised through the **real path**: host encode → copy the
buffer into a wasmtime guest's linear memory → guest reads + traverses (node count + sum of
span byte-lengths). All three encodings produced the **same checksum** (correctness
cross-check). `copy + guest read` is averaged over 300 iterations; `host encode` over 200.

- `rkyv (unchecked)` — `access_unchecked`: the **host-written-AST read** path (the spec's
  trusted carve-out).
- `rkyv (checked)` — `access` + `bytecheck`: the **untrusted-output** path (shown for cost).
- `msgpack` — `rmp-serde`, which must **fully deserialize** before traversal (not zero-copy).
- `flatbuffers (verified)` — `root_as_ast` with buffer verification; vtable/struct access.

## Result (100k nodes; µs per file)

| encoding | buffer bytes | host encode | copy + guest read | real-path total |
| --- | ---: | ---: | ---: | ---: |
| **rkyv (unchecked)** | 5,885,728 | 1102 | **182** | **1284** |
| rkyv (checked / bytecheck) | 5,885,728 | 1102 | 3701 | 4803 |
| msgpack | 6,526,361 | 4315 | 10172 | 14487 |
| flatbuffers (verified) | 5,885,740 | 2307 | 3634 | 5941 |

Real-path speedup vs `rkyv (unchecked)`: **msgpack ≈ 11.3×**, **flatbuffers ≈ 4.6×**.
The rkyv buffer is 0.90× the msgpack size and ~1.00× the flatbuffers size.

## Findings

- **rkyv wins decisively.** The go/no-go criterion (adopt rkyv iff ≥ 1.5× over MsgPack on
  the real path) is met by a wide margin (≈ 11.3×), dominated by the **read side**:
  zero-copy pointer-cast access (182 µs) vs MsgPack's full deserialize (10.2 ms) and
  FlatBuffers' verified vtable access (3.6 ms).
- **`bytecheck` is O(N) and costly** (~3.5 ms for the full 100k-node AST). This validates
  the ABI policy: the **host-written AST is read with `access_unchecked`** (trusted, 182 µs),
  and **checked `access` is reserved for untrusted plugin _output_**. Bytechecking the whole
  AST per plugin would erase most of rkyv's advantage (rkyv-checked at 4.8 ms is still
  fastest, but only ~3× rather than ~11×).
- Even with verification, **rkyv-checked (4.8 ms) < FlatBuffers-verified (5.9 ms) <
  msgpack (14.5 ms)** — rkyv is fastest in every mode, and produces the smallest buffer.

## Caveats

- Synthetic AST + single machine; absolute numbers will vary. The decision rests on the
  large, consistent ratios.
- FlatBuffers was measured with buffer verification; an unchecked FB read would be faster
  but remains vtable-indirected per access (no zero-copy struct mapping like rkyv).
- Multi-language note: rkyv's archived layout is Rust-specific, so a future non-Rust PDK
  would need its own rkyv reader (design Path A). This spike only gates the v1 (Rust-only)
  encoding choice; the frozen-core/`bytecheck` scaffolding is kept encoding-aware so the
  contract is documented in [`../design/abi-spec.md`](../design/abi-spec.md).

Harness: a standalone `astdef` / `guest` (wasm32) / `host` (wasmtime) workspace, kept
outside the product tree; available to reproduce these numbers or to seed the M1 `bench`
job.
