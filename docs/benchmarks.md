# Benchmarks — TsuzuLint vs other linters

> Status: **measured (0.1.0, R17 gate).** The headline comparison below — TsuzuLint vs Node
> `textlint` on the **same corpus and the same full `ja-technical-writing` rule set** — is the
> release gate for `0.1.0` (issue #57). Internal technology-selection research (e.g. the
> plugin-transport encoding choice) lives under [`research/`](research/encoding-spike.md), not here.

This page is for people evaluating TsuzuLint: *if I switch to this, how much faster is it, and at
what memory cost, compared to what I use today?*

## What we measure

- The **same corpus** and an **equivalent rule set** across every tool.
- Wall-clock **throughput** (docs/s, MiB/s), **cold vs warm** runs (cache on/off), and **peak memory**.
- Every result is recorded together with the **tool versions, the measurement date, and the
  environment** (hardware + OS). Timing is machine- and version-dependent and is not meaningful
  without those conditions — the environment block is part of the result, not a footnote.

The harness, corpus, and configs that produce these numbers live in [`../bench/`](../bench/); see
[`../bench/README.md`](../bench/README.md) to reproduce them.

## TsuzuLint vs textlint — the full `ja-technical-writing` preset

Both tools run the complete `textlint-rule-preset-ja-technical-writing` (23 rules) over an identical
corpus of **30 original Japanese technical-writing documents (2.831 MiB)**, generated deterministically
by [`bench/gen_corpus.py`](../bench/gen_corpus.py) (the generator is committed; its output is not). This is an
apples-to-apples comparison across the **whole** preset — not a matchable subset — including the
morphology-backed rules: TsuzuLint provisions IPADIC via its `morphology` config block, so
`no-doubled-joshi`, `no-mix-dearu-desumasu`, `no-doubled-conjunction`, `ja-no-successive-word`, … all
fire exactly as the preset intends.

One process lints the whole corpus in a single invocation, so per-file startup is amortized the way a
real `tzlint lint .` / `textlint .` run amortizes it.

### Throughput (steady state)

| Tool (config) | Mean wall | Throughput | docs/s |
| --- | ---: | ---: | ---: |
| **TsuzuLint** (parse + lint every file, `--no-cache`) | **2.52 s** | **1.12 MiB/s** | **11.9** |
| TsuzuLint (warm document cache) | 2.57 s | 1.10 MiB/s | 11.7 |
| textlint (no persistent cache) | 37.33 s | 0.076 MiB/s | 0.80 |
| textlint (`--cache`, warm) | 38.65 s | 0.073 MiB/s | 0.78 |

**TsuzuLint lints this preset ≈ 14.8× faster than textlint** (2.52 s vs 37.33 s; full parse + lint,
neither side taking a cache shortcut).

TsuzuLint's persistent document cache (`.tzlintcache`) gives **no** speedup here, by design: the 30
corpus files are byte-unique, so nothing is a cache hit — the cache pays off only on *re-linting
unchanged files*, which this batch deliberately avoids. textlint's `--cache` is likewise no faster on
this all-unique corpus.

### Cold start (every cache cleared before each run)

| Tool | Mean wall | vs textlint |
| --- | ---: | ---: |
| **TsuzuLint** (provision dictionary + lint) | **2.63 s** | **14.4× faster** |
| textlint | 37.85 s | — |

TsuzuLint's cold start adds only ~0.1 s over warm: decompressing and loading the 13 MiB IPADIC
container in memory is cheap, so there is effectively no cold-start penalty — the single binary is
ready to lint immediately, with no Node module graph to load.

### Peak memory

| Tool | Peak RSS |
| --- | ---: |
| **TsuzuLint** | **255 MiB** |
| textlint | 1013 MiB |

TsuzuLint holds the entire IPADIC dictionary in memory and still uses **≈ 4× less RAM** than
textlint on the same workload.

### Environment & versions

| | |
| --- | --- |
| Date | 2026-06-17 |
| Machine | Apple M1 Pro (8 performance + 2 efficiency cores), 32 GiB RAM |
| OS | macOS 26.2 (arm64) |
| TsuzuLint | `0.1.0`, release build (`cargo build --release`), `rustc 1.96.0`; morphology: lindera 3.0.7 + embedded IPADIC |
| textlint | `15.7.1` + `textlint-rule-preset-ja-technical-writing` `12.0.2`, Node.js `v24.13.0` |
| Tooling | hyperfine 1.20.0 (warmup 3, 10 runs for throughput; 5 runs for cold), `/usr/bin/time -l` for peak RSS |
| Corpus | `bench/corpus/` — 30 documents, 2.831 MiB, 2 969 224 bytes |

### Apples-to-apples: three honest caveats

The corpus, the 23 preset rules, and their thresholds match on both sides. Three deliberate
differences are called out so the comparison stays honest (details in
[`../bench/README.md`](../bench/README.md)):

1. **`no-mixed-zenkaku-hankaku-alphabet` is disabled** for TsuzuLint — it is a TsuzuLint-original rule
   with no upstream-preset counterpart, so disabling it keeps both tools on the same 23 rules.
2. **`no-mix-dearu-desumasu` options differ** — upstream pins
   `preferInBody:"ですます" / preferInList:"である"`; TsuzuLint's rule auto-detects the majority style.
   Both run the rule; the exact flagging differs slightly.
3. **`prh` is excluded** — it is project-specific (needs a term dictionary) and not part of the
   preset, so neither side runs it in the core comparison.

## Other tools

`markdownlint` (Markdown structure), `redpen`, and `vale` will be added once a fair, equivalent rule
set exists on each side. They are out of scope for the 0.1.0 gate, which is specifically about the
Japanese `textlint` preset that TsuzuLint replaces.
