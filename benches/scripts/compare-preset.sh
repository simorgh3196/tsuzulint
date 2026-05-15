#!/usr/bin/env bash
# Compare tzlint (native, ja-technical-writing preset) vs textlint
# (textlint-rule-preset-ja-technical-writing) using hyperfine.
#
# This is a bigger-surface-area comparison than `compare.sh`:
# the tsuzulint side enables ~9 native rules via the preset, and the textlint
# side enables the equivalent (more) rules via the official preset.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHES_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${BENCHES_DIR}/.." && pwd)"

TZLINT_BIN="${REPO_ROOT}/target/release/tzlint"
TZLINT_CONFIG="${BENCHES_DIR}/configs/tsuzulint-preset/.tsuzulint.jsonc"
TEXTLINT_DIR="${BENCHES_DIR}/configs/textlint-preset"

if [[ ! -x "${TZLINT_BIN}" ]]; then
  echo "tzlint release binary not found; run benches/scripts/setup.sh first." >&2
  exit 1
fi

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "hyperfine not found; install via 'brew install hyperfine' or 'cargo install hyperfine'." >&2
  exit 1
fi

if [[ ! -x "${TEXTLINT_DIR}/node_modules/.bin/textlint" ]]; then
  echo "textlint preset harness not installed. Run:" >&2
  echo "  (cd ${TEXTLINT_DIR} && npm install)" >&2
  exit 1
fi

mkdir -p "${BENCHES_DIR}/results"

WARMUP="${WARMUP:-2}"
RUNS="${RUNS:-5}"

run_size() {
  local size="$1"
  local corpus="${BENCHES_DIR}/corpus/${size}"

  if [[ ! -d "${corpus}" ]]; then
    echo "Corpus ${corpus} missing. Run gen-corpus.sh." >&2
    return 1
  fi

  local -a files=()
  while IFS= read -r -d '' f; do
    files+=("${f}")
  done < <(find "${corpus}" -name '*.md' -print0)
  local n_files="${#files[@]}"

  local result_md="${BENCHES_DIR}/results/preset-${size}.md"
  local result_json="${BENCHES_DIR}/results/preset-${size}.json"

  echo "==> Benchmarking (preset, ${size}) ${n_files} files"

  hyperfine \
    --warmup "${WARMUP}" \
    --runs "${RUNS}" \
    --export-markdown "${result_md}" \
    --export-json "${result_json}" \
    --command-name "tzlint (preset:ja-technical-writing)" \
    "'${TZLINT_BIN}' --no-cache -c '${TZLINT_CONFIG}' lint $(printf "'%s' " "${files[@]}") --format text >/dev/null 2>&1 || true" \
    --command-name "textlint (preset-ja-technical-writing)" \
    "cd '${TEXTLINT_DIR}' && ./node_modules/.bin/textlint '${corpus}' --format compact >/dev/null 2>&1 || true"
}

SIZES=("${@}")
if [[ "${#SIZES[@]}" -eq 0 ]]; then
  SIZES=(small medium large)
fi

for size in "${SIZES[@]}"; do
  run_size "${size}"
done

echo
echo "Results written to ${BENCHES_DIR}/results/preset-*.{md,json}"
