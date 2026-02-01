# Texide Benchmark Suite

Performance comparison between Texide and textlint.

## Overview

This benchmark suite measures and compares the performance of Texide against textlint using identical rules.

## Structure

```
benches/
├── corpus/                # Test data files (generated on-demand, NOT in git)
│   ├── large_single.md   # ~100MB single file
│   └── many_files/       # 1000 small files
├── rules/                 # Rule implementations for comparison
│   ├── texide/           # Texide WASM rules (uses existing rules/no-todo)
│   └── textlint/         # textlint JS rules (custom implementation)
├── scripts/               # Benchmark scripts
│   ├── corpus_generator.rs    # Generates test corpus
│   ├── run.sh                 # Full benchmark runner
│   ├── run-quick.sh           # Quick benchmark (for CI)
│   └── generate-report.js     # Results aggregation
└── results/               # Benchmark results (generated, NOT in git)
```

## Prerequisites

- Rust toolchain (1.92+)
- Node.js (20+)
- npm

## Usage

### 1. Generate Test Corpus

The corpus is generated on-demand and NOT committed to git (see `.gitignore`).

```bash
# Generate ~100MB single file and 1000 small files
cargo run --bin corpus-generator --release
```

This creates:
- `benches/corpus/large_single.md` (~100MB)
- `benches/corpus/many_files/` (1000 files, doc_0001.md through doc_1000.md)

### 2. Run Full Benchmark

```bash
# Run complete benchmark suite
./benches/scripts/run.sh
```

This measures:
- **Cold Start**: First run with no cache
- **Warm Run**: Subsequent runs with cache
- **Memory Usage**: Peak memory consumption
- **Execution Time**: Total processing time

Results are saved to `benches/results/benchmark-<timestamp>.json`.

### 3. View Results

```bash
# Latest results
cat benches/results/$(ls -t benches/results/ | head -1)

# Or pretty-print with jq
cat benches/results/benchmark-*.json | jq
```

## Scenarios

### Scenario 1: Large Single File (100MB)

Tests parsing and linting performance on a single large markdown file.

### Scenario 2: Many Small Files (1000 files)

Tests parallel processing and file I/O performance.

## Metrics

| Metric | Description |
|--------|-------------|
| Cold Start | Time for first run (no cache, WASM load) |
| Warm Run | Time for subsequent runs (cached) |
| Peak Memory | Maximum resident memory (KB) |
| Speedup | textlint time / Texide time |

## CI Integration

Benchmarks run automatically on PRs that modify performance-critical code.

### Manual Trigger

```bash
gh workflow run benchmark.yml
```

### Results in PR Comments

Benchmark results are automatically posted as PR comments.

## Comparing Rules

Both implementations use the `no-todo` rule:

- **Texide**: `rules/no-todo` (WASM, Rust)
- **textlint**: `benches/rules/textlint/lib/no-todo.js` (JS)

The rules are intentionally simple and identical in functionality:
- Detect `TODO:`, `FIXME:`, `XXX:` markers
- Report diagnostics with position information

## Notes

- Generated corpus files are cached in CI to avoid regeneration
- Large files are excluded from git via `.gitignore`
- Results are uploaded as artifacts and can be compared across runs

## Development

To add a new benchmark scenario:

1. Add corpus generation logic to `corpus_generator.rs`
2. Add measurement logic to `run.sh`
3. Update `generate-report.js` to include new metrics
4. Update this README
