# Benchmark: tsuzulint vs textlint (R17)

The release gate for `0.1.0` (issue #57, sub-step **R17**): does tsuzulint lint Japanese prose
faster than Node `textlint` on the **same corpus** and the **same full rule set**?

Both tools run the complete `textlint-rule-preset-ja-technical-writing` (23 rules) over an identical
committed corpus, so the comparison is apples-to-apples across the whole preset — not a "matchable
subset". The headline numbers and the environment block live in
[`../docs/benchmarks.md`](../docs/benchmarks.md); this directory holds the harness that produces them.

## Layout

The **generator** is the committed source of truth; the corpus it emits is a regenerable artifact, so
it is gitignored — just like the dictionary container. The generator is deterministic, so the bytes
are identical on every machine.

| Path | Committed? | What |
| --- | --- | --- |
| `gen_corpus.py` | ✅ | Deterministic generator for the corpus (seeded; byte-unique files so no linter cache dedups them). The committed source of truth. |
| `.tzlintrc.json` | ✅ | tsuzulint config (the canonical config name): `extends ja-technical-writing` + morphology dictionary + the parity caveats below. |
| `textlint/.textlintrc.json` | ✅ | textlint config: the full `preset-ja-technical-writing`. |
| `textlint/package.json` + `package-lock.json` | ✅ | Pinned textlint + preset versions (lockfile pins transitive deps). |
| `build-dict.sh` | ✅ | Rebuilds the IPADIC dictionary container locally. |
| `run.sh` | ✅ | The hyperfine harness: throughput, cold/warm, peak RSS. Regenerates the corpus if absent. |
| `corpus/` | ❌ (gitignored) | The 30-document corpus (~2.8 MiB) — regenerated from `gen_corpus.py`, never committed. |
| `ipadic.dict.zst` | ❌ (gitignored) | The ~13 MiB dictionary container — rebuilt locally, never committed. |
| `textlint/node_modules/` | ❌ (gitignored) | `npm install` output. |
| `results/`, `.tzlint*` | ❌ (gitignored) | Run artifacts and linter caches. |

## Setup

```sh
# 1. Build the release linter
cargo build --release -p tzlint_cli

# 2. Build the IPADIC dictionary container (compiles lindera's embedded IPADIC; slow once).
#    Requires the `zstd` CLI and `b3sum` (brew install b3sum / cargo install b3sum).
bench/build-dict.sh
#    -> writes bench/ipadic.dict.zst and prints its BLAKE3 pin.
#       If the pin differs from `morphology.pin` in bench/.tzlintrc.json (e.g. after a
#       lindera bump), update that value to match.

# 3. Install the textlint side (pinned versions)
(cd bench/textlint && npm install)

# 4. Generate the corpus (deterministic; gitignored, so generate it once).
#    run.sh also does this automatically if corpus/ is absent.
python3 bench/gen_corpus.py
```

## Run

```sh
bench/run.sh                 # needs hyperfine, jq, and bc on PATH
# BENCH_RUNS=20 BENCH_WARMUP=5 bench/run.sh   # more samples
```

## What is measured

One process lints the **whole corpus** in a single invocation, so per-file startup is amortized the
way a real `tzlint lint .` / `textlint .` run amortizes it.

- **Throughput** — MiB/s and docs/s from the steady-state mean wall time (hyperfine, warmup + N runs).
- **Cold vs warm** — *cold* clears every persistent cache before each run (tsuzulint's `.tzlintcache`
  document cache and `.tzlint/dict/` dictionary cache; textlint's `--cache` file); *warm* keeps them.
  tsuzulint's `--no-cache` row is the honest "parse + lint every file from scratch" number.
- **Peak RSS** — `maximum resident set size` from macOS `/usr/bin/time -l`, one full run each.

## Apples-to-apples: the rule set, and three honest caveats

The corpus, the 23 preset rules, and their thresholds match on both sides. Three deliberate
differences are called out so the comparison stays honest:

1. **`no-mixed-zenkaku-hankaku-alphabet` is disabled** for tsuzulint in `.tzlintrc.json`. It is a
   tsuzulint-original rule with no upstream-preset counterpart, so disabling it keeps both tools on
   the same 23 rules.
2. **`no-mix-dearu-desumasu` options differ.** Upstream's preset passes
   `preferInBody:"ですます" / preferInList:"である" / preferInHeader:"" / strict:false`; tsuzulint's
   rule auto-detects the majority style (the body/list/header distinction is not modeled yet). Both
   run the rule; the exact flagging differs slightly.
3. **`prh` is excluded.** `textlint-rule-prh` / tsuzulint's `ja-prh` is project-specific (it needs a
   term dictionary) and is not part of the preset, so neither side runs it in the core comparison.

tsuzulint's morphology-backed rules (`no-doubled-joshi`, `no-mix-dearu-desumasu`,
`no-doubled-conjunction`, `ja-no-successive-word`, …) are **live** here: the IPADIC dictionary is
provisioned via the `morphology` config block, so they fire exactly as the preset intends — the
comparison includes the morphology work, not just the surface rules.
