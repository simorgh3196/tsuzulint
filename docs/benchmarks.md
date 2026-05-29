# Benchmarks — TsuzuLint vs other linters

> Status: template (M1). How TsuzuLint compares to other natural-language / Markdown
> linters on the **same corpus and rule set**. Internal technology-selection research
> (e.g. the plugin-transport encoding choice) lives under
> [`research/`](research/encoding-spike.md), not here.

This page is for people evaluating TsuzuLint: *if I switch to this, how much faster is it,
and at what memory cost, compared to what I use today?*

## What we measure

- The **same corpus** and an **equivalent rule set** across every tool.
- Wall-clock **throughput** (files/s, MB/s), **cold vs warm** runs (cache on/off), and
  **peak memory**.
- Every result is recorded together with the **tool versions, the measurement date, and the
  environment** (hardware + OS). Timing is machine- and version-dependent and is not
  meaningful without those conditions — see the environment block convention used in
  [`research/encoding-spike.md`](research/encoding-spike.md).

## Tools compared

- **textlint** (Node.js) with a comparable Japanese preset.
- **markdownlint** (Node.js).
- Others where a fair, equivalent rule set exists (e.g. redpen, vale).

## Results

_TODO (M1+):_ populated once the lean core lints real corpora. TsuzuLint targets large
speedups over Node-based linters (notably textlint); the figures will be **substantiated
here with full environment metadata**, not asserted. Each table will carry its own
"Environment & versions" block so a reader can judge whether the numbers apply to them.
