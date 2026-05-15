#!/usr/bin/env bash
# Compare tzlint (native rule engine) vs tzlint (WASM rule pipeline)
# vs textlint across the generated corpus using hyperfine.
#
# Assumes benches/scripts/setup.sh has been run, so:
#   - target/release/tzlint exists
#   - benches/configs/tsuzulint/rules/ has fresh WASM rule manifests
#   - benches/corpus/{small,medium,large,monolithic}/ exists
#   - benches/configs/textlint/node_modules/ is populated
#
# Compares three commands per corpus size:
#   1. tzlint native rules (built-in Rust rules)
#   2. tzlint WASM rules   (loaded from rules/no-todo/...)
#   3. textlint            (reference implementation)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHES_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${BENCHES_DIR}/.." && pwd)"

TZLINT_BIN="${REPO_ROOT}/target/release/tzlint"
TZLINT_NATIVE_CFG="${BENCHES_DIR}/configs/tsuzulint-native/.tsuzulint.jsonc"
TZLINT_WASM_CFG="${BENCHES_DIR}/configs/tsuzulint/.tsuzulint.jsonc"
TEXTLINT_DIR="${BENCHES_DIR}/configs/textlint"

if [[ ! -x "${TZLINT_BIN}" ]]; then
  echo "tzlint release binary not found at ${TZLINT_BIN}. Run benches/scripts/setup.sh first." >&2
  exit 1
fi

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "hyperfine not found. Install via 'brew install hyperfine' or 'cargo install hyperfine'." >&2
  exit 1
fi

if [[ ! -x "${TEXTLINT_DIR}/node_modules/.bin/textlint" ]]; then
  echo "textlint not installed. Run benches/scripts/setup.sh first." >&2
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

  if [[ "${n_files}" -eq 0 ]]; then
    echo "No .md files in ${corpus}; skipping" >&2
    return 0
  fi

  local result_md="${BENCHES_DIR}/results/wasm-${size}.md"
  local result_json="${BENCHES_DIR}/results/wasm-${size}.json"

  echo "==> Benchmarking ${size} (${n_files} files) native vs WASM vs textlint"

  hyperfine \
    --warmup "${WARMUP}" \
    --runs "${RUNS}" \
    --export-markdown "${result_md}" \
    --export-json "${result_json}" \
    --command-name "tzlint (native)" \
    "'${TZLINT_BIN}' --no-cache -c '${TZLINT_NATIVE_CFG}' lint $(printf "'%s' " "${files[@]}") --format text >/dev/null 2>&1 || true" \
    --command-name "tzlint (WASM)" \
    "'${TZLINT_BIN}' --no-cache -c '${TZLINT_WASM_CFG}' lint $(printf "'%s' " "${files[@]}") --format text >/dev/null 2>&1 || true" \
    --command-name "textlint" \
    "cd '${TEXTLINT_DIR}' && ./node_modules/.bin/textlint '${corpus}' --format compact >/dev/null 2>&1 || true"
}

SIZES=("${@}")
if [[ "${#SIZES[@]}" -eq 0 ]]; then
  SIZES=(small medium large monolithic)
fi

for size in "${SIZES[@]}"; do
  run_size "${size}"
done

echo
echo "Results written to ${BENCHES_DIR}/results/wasm-*.{md,json}"
ls -1 "${BENCHES_DIR}/results/" | grep -E "^wasm-"
