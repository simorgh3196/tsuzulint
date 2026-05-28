# Benchmarks

> Status: template (M0). criterion over real corpora, including the WASM path.

- **M0 encoding spike:** rkyv vs MsgPack vs FlatBuffers on a ~10⁵-node AST through the
  **real** path (host encode + host→guest copy + plugin read + output `bytecheck`).
  Recorded here before the ABI is frozen. Go/no-go: adopt rkyv only if ≥1.5× over MsgPack.
- Regression thresholds warn in CI (`bench` job: smoke on PR, full on `main`).
- Bench corpus includes the WASM path, **instance churn**, and one **pathologically large
  single file** (the no-intra-file-parallelism case).
