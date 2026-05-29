# Benchmarks

> criterion over real corpora (incl. the WASM path, instance churn, and one pathologically
> large single file) is added with M1's real code via the CI `bench` job. This file
> currently records the **M0 encoding spike** that gated the plugin-transport choice.

## M0 encoding spike — rkyv vs MsgPack vs FlatBuffers (real wasmtime path)

**Decision: GO — adopt `rkyv`** for the plugin transport. The encoding is no longer
provisional.

### Method

A representative index-based AST of **100,000 nodes** over a ~3.49 MB CJK+ASCII
markdown-like text (`Vec<Node>` + an owned `String`, the frozen `AstCoreV1` shape) was
encoded with each format and exercised through the **real path**: host encode → copy the
buffer into a wasmtime guest's linear memory → guest reads + traverses (node count + sum of
span byte-lengths). All three encodings produced the **same checksum** (correctness
cross-check). Single machine (Apple Silicon, rustc 1.93, wasmtime 45, release builds);
numbers are order-of-magnitude, not a committed benchmark.

- `rkyv (unchecked)` — `access_unchecked`: the spec's **host-written-AST read** path.
- `rkyv (checked)` — `access` + `bytecheck`: the **untrusted-output** path (shown for cost).
- `msgpack` — `rmp-serde`, which must **fully deserialize** before traversal (not zero-copy).
- `flatbuffers (verified)` — `root_as_ast` with buffer verification; vtable/struct access.

### Result (100k nodes; µs per file)

| encoding | buffer bytes | host encode | copy + guest read | real-path total |
| --- | ---: | ---: | ---: | ---: |
| **rkyv (unchecked)** | 5,885,728 | 1040 | **192** | **1232** |
| rkyv (checked / bytecheck) | 5,885,728 | 1040 | 3730 | 4770 |
| msgpack | 6,526,361 | 4070 | 10169 | 14239 |
| flatbuffers (verified) | 5,885,740 | 2334 | 3875 | 6209 |

Real-path speedup vs `rkyv (unchecked)`: **msgpack ≈ 11.6×**, **flatbuffers ≈ 5.0×**.
The rkyv buffer is 0.90× the msgpack size and ~1.00× the flatbuffers size.

### Findings

- **rkyv wins decisively.** The go/no-go criterion (adopt rkyv iff ≥ 1.5× over MsgPack on
  the real path) is met by a wide margin (≈ 11.6×). The win is dominated by the **read
  side**: zero-copy pointer-cast access (192 µs) vs MsgPack's full deserialize (10.2 ms)
  and FlatBuffers' verified vtable access (3.9 ms).
- **`bytecheck` is O(N) and costly** (~3.5 ms for the full 100k-node AST). This validates
  the ABI policy: the **host-written AST is read with `access_unchecked`** (trusted, 192
  µs), and **checked `access` is reserved for untrusted plugin _output_**. Bytechecking the
  whole AST once per plugin would erase most of rkyv's advantage (rkyv-checked at 4.77 ms is
  still fastest, but only ~3× rather than ~11×).
- Even with verification, **rkyv-checked (4.77 ms) < FlatBuffers-verified (6.21 ms) <
  msgpack (14.24 ms)** — rkyv is fastest in every mode, and produces the smallest buffer.

### Caveats

- Synthetic AST + single machine; absolute numbers will vary. The decision rests on the
  large, consistent ratios, not the absolutes.
- FlatBuffers was measured with buffer verification; an unchecked FB read would be faster
  but remains vtable-indirected per access (no zero-copy struct mapping like rkyv).
- Multi-language note: rkyv's archived layout is Rust-specific, so a future non-Rust PDK
  would need its own rkyv reader (design Path A). This spike only gates the v1 (Rust-only)
  encoding choice; the frozen-core/`bytecheck` scaffolding is kept encoding-aware so the
  contract is documented in `abi-spec.md`.

Harness: a standalone `astdef` / `guest` (wasm32) / `host` (wasmtime) workspace, kept
outside the product tree; available to reproduce these numbers or to seed the M1 `bench`
job.
