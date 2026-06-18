#!/usr/bin/env bash
# tsuzulint vs textlint throughput benchmark over the same Japanese corpus and the same full
# `ja-technical-writing` rule set (R17, issue #57). Run from anywhere; paths resolve to bench/.
#
# Prerequisites (see bench/README.md):
#   - a release tzlint binary:        cargo build --release -p tzlint_cli
#   - the IPADIC dictionary container: bench/build-dict.sh   (produces bench/ipadic.dict.zst)
#   - the textlint side installed:     (cd bench/textlint && npm install)
#   - hyperfine and jq on PATH
#
# Measures, for both tools, a single process linting the whole corpus:
#   - throughput (MiB/s, docs/s) from steady-state wall time
#   - cold (caches cleared) vs warm (persistent caches populated)
#   - peak resident set size (macOS `/usr/bin/time -l`)
# Results land in bench/results/ (gitignored).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

tzlint="$here/../target/release/tzlint"
textlint="$here/textlint/node_modules/.bin/textlint"
corpus="corpus"
results="$here/results"
runs="${BENCH_RUNS:-10}"
warmup="${BENCH_WARMUP:-3}"

# --- prerequisite checks -------------------------------------------------------------------------
fail() { echo "error: $1" >&2; exit 1; }
[ -x "$tzlint" ]   || fail "tzlint release binary not found at $tzlint (run: cargo build --release -p tzlint_cli)"
[ -f "ipadic.dict.zst" ] || fail "ipadic.dict.zst missing (run: bench/build-dict.sh)"
[ -x "$textlint" ] || fail "textlint not installed (run: cd bench/textlint && npm install)"
command -v hyperfine >/dev/null || fail "hyperfine not on PATH"
command -v jq >/dev/null || fail "jq not on PATH"
command -v bc >/dev/null || fail "bc not on PATH"
# The corpus is a regenerable artifact (gitignored). Generate it deterministically if absent.
if ! ls "$corpus"/*.md >/dev/null 2>&1; then
  command -v python3 >/dev/null || fail "no corpus and python3 not on PATH to generate it"
  echo "==> corpus/ not found; generating it deterministically (python3 gen_corpus.py)"
  python3 "$here/gen_corpus.py"
fi
mkdir -p "$results"

# --- corpus stats --------------------------------------------------------------------------------
corpus_bytes=$(cat "$corpus"/*.md | wc -c | tr -d ' ')
corpus_files=$(ls "$corpus"/*.md | wc -l | tr -d ' ')
corpus_mib=$(echo "scale=3; $corpus_bytes / 1048576" | bc)
echo "corpus: $corpus_files files, $corpus_bytes bytes (${corpus_mib} MiB)"

# --- commands ------------------------------------------------------------------------------------
# tzlint: dict cache is warmed once below so we measure LINT throughput, not one-time dict
# provisioning. `--no-cache` is the honest "full parse+lint every file" number; the default (warm
# .tzlintcache) is the repeat-run number.
tz_full="$tzlint lint $corpus -c .tzlintrc.json -f json >/dev/null"
tz_warm="$tzlint lint $corpus -c .tzlintrc.json -f json >/dev/null"
# textlint: default has no persistent cache (cold every run); --cache is the repeat-run number.
tx_full="$textlint --config textlint/.textlintrc.json -f json $corpus/*.md >/dev/null"
tx_warm="$textlint --config textlint/.textlintrc.json --cache --cache-location $results/.textlintcache -f json $corpus/*.md >/dev/null"

# Warm the tzlint dictionary cache (decompress+load once → .tzlint/dict/<pin>.dict).
rm -rf .tzlint .tzlintcache "$results/.textlintcache"
eval "$tz_full" 2>/dev/null || true

echo
echo "=== throughput (steady state, warmup=$warmup runs=$runs) ==="
# tzlint --no-cache: full parse+lint every file, no doc-cache shortcut (dict cache warm).
# tzlint warm: persistent .tzlintcache populated → unchanged files skip parse+lint.
# textlint: default (no persistent cache).
# Each hyperfine cell ignores the linters' non-zero "found issues" exit (-i).
hyperfine -i --warmup "$warmup" --runs "$runs" --export-json "$results/throughput.json" \
  --command-name "tzlint (no-cache)"  --prepare "rm -f .tzlintcache" "$tzlint lint $corpus -c .tzlintrc.json --no-cache -f json >/dev/null" \
  --command-name "tzlint (warm cache)" "$tz_warm" \
  --command-name "textlint (no-cache)" "$tx_full" \
  --command-name "textlint (warm cache)" "$tx_warm"

# --- cold runs (caches cleared) ------------------------------------------------------------------
echo
echo "=== cold start (caches cleared before each run) ==="
hyperfine -i --warmup 0 --runs 5 --export-json "$results/cold.json" \
  --command-name "tzlint (cold: provision dict + lint)" \
  --prepare "rm -rf .tzlint .tzlintcache" \
  "$tzlint lint $corpus -c .tzlintrc.json -f json >/dev/null" \
  --command-name "textlint (cold)" \
  --prepare "rm -rf $results/.textlintcache" \
  "$tx_full"

# --- peak RSS ------------------------------------------------------------------------------------
echo
echo "=== peak resident set size (one full run each) ==="
rm -f .tzlintcache
/usr/bin/time -l bash -c "$tz_full" 2>"$results/tz.rss" || true
/usr/bin/time -l bash -c "$tx_full" 2>"$results/tx.rss" || true
tz_rss=$(grep "maximum resident set size" "$results/tz.rss" | awk '{print $1}')
tx_rss=$(grep "maximum resident set size" "$results/tx.rss" | awk '{print $1}')
echo "tzlint   peak RSS: $tz_rss bytes ($(echo "scale=1; $tz_rss/1048576" | bc) MiB)"
echo "textlint peak RSS: $tx_rss bytes ($(echo "scale=1; $tx_rss/1048576" | bc) MiB)"

# --- summary -----------------------------------------------------------------------------------
echo
echo "=== summary (MiB/s, docs/s from steady-state mean) ==="
jq -r --argjson bytes "$corpus_bytes" --argjson files "$corpus_files" '
  .results[] |
  [ .command,
    (.mean | (. * 1000 | round / 1000) | tostring) + "s",
    (($bytes / 1048576) / .mean | . * 100 | round / 100 | tostring) + " MiB/s",
    ($files / .mean | . * 10 | round / 10 | tostring) + " docs/s"
  ] | @tsv' "$results/throughput.json" | column -t -s $'\t'

echo
echo "results JSON: $results/throughput.json , $results/cold.json"
