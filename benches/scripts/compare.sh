#!/usr/bin/env bash
# Compare tzlint (native-rule mode) vs textlint across the generated corpus
# using hyperfine.
#
# Assumes benches/scripts/setup.sh has been run. Writes Markdown + JSON
# results to benches/results/.
#
# Why native-only? The WASM plugin dispatch pipeline currently has a
# dispatch-level bug where rules that filter on `node.type == "Str"` never
# receive Str nodes (they get blocks instead) and therefore return zero
# diagnostics. That would make a WASM-vs-textlint comparison meaningless.
# Measuring native is what actually matches the textlint behaviour today,
# and pragmatically it is also where we want to be — native is how we
# outrun textlint on speed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHES_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${BENCHES_DIR}/.." && pwd)"

TZLINT_BIN="${REPO_ROOT}/target/release/tzlint"
TZLINT_NATIVE_DIR="${BENCHES_DIR}/configs/tsuzulint-native"
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

  local result_md="${BENCHES_DIR}/results/${size}.md"
  local result_json="${BENCHES_DIR}/results/${size}.json"

  echo "==> Benchmarking ${size} (${n_files} files)"

  # tzlint accepts multiple explicit paths as patterns, but passing 100+ long
  # absolute paths on the command line gets unwieldy. Instead, lean on the
  # walker: pass the corpus root directory to tzlint via a CLI pattern, with
  # the config's include list pinned to "**/*.md".
  local include_cfg="${BENCHES_DIR}/configs/tsuzulint-native/.tsuzulint.jsonc"
  hyperfine \
    --warmup "${WARMUP}" \
    --runs "${RUNS}" \
    --export-markdown "${result_md}" \
    --export-json "${result_json}" \
    --command-name "tzlint (native)" \
    "'${TZLINT_BIN}' --no-cache -c '${include_cfg}' lint $(printf "'%s' " "${files[@]}") --format compact >/dev/null 2>&1 || true" \
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
echo "Results written to ${BENCHES_DIR}/results/"
ls -1 "${BENCHES_DIR}/results/"
