#!/usr/bin/env bash
# One-shot setup for the benchmark harness.
#
# 1. Builds the tzlint release binary.
# 2. Builds the WASM rules.
# 3. Copies each canonical rules/<name>/tsuzulint-rule.json to a bench-local
#    location, updating the WASM path + hash to point at the freshly built
#    artifact. This keeps the canonical manifests (with their pinned hashes)
#    untouched while letting the bench use a local, self-contained copy.
# 4. Regenerates the corpus.
# 5. Installs the textlint dependencies for the comparison side.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHES_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${BENCHES_DIR}/.." && pwd)"

echo "==> Building tzlint release binary"
(cd "${REPO_ROOT}" && cargo build --release --bin tzlint)

echo "==> Building WASM rules"
(cd "${REPO_ROOT}/rules" && cargo build --target wasm32-wasip1 --release)

echo "==> Writing bench-local rule manifests"
TARGET_DIR="${REPO_ROOT}/rules/target/wasm32-wasip1/release"
RULES_DIR="${BENCHES_DIR}/configs/tsuzulint/rules"
rm -rf "${RULES_DIR}"
mkdir -p "${RULES_DIR}"

copy_manifest() {
  local rule_name="$1"
  local wasm_stem="$2"

  local rule_dir="${RULES_DIR}/${rule_name}"
  mkdir -p "${rule_dir}"

  local wasm_src="${TARGET_DIR}/${wasm_stem}.wasm"
  local wasm_dst="${rule_dir}/rule.wasm"
  cp "${wasm_src}" "${wasm_dst}"

  local hash
  hash="$(shasum -a 256 "${wasm_dst}" | awk '{print $1}')"

  # Start from the canonical manifest so we inherit isolation_level,
  # languages, capabilities, node_types, etc. Then rewrite the single wasm
  # entry to point at the co-located rule.wasm with the fresh hash.
  local canonical="${REPO_ROOT}/rules/${rule_name}/tsuzulint-rule.json"
  python3 - "${canonical}" "${hash}" > "${rule_dir}/tsuzulint-rule.json" <<'PY'
import json, sys
path, new_hash = sys.argv[1], sys.argv[2]
with open(path, 'r', encoding='utf-8') as f:
    manifest = json.load(f)
manifest['wasm'] = [{'path': 'rule.wasm', 'hash': new_hash}]
# Point $schema at the repo-relative path so the bench dir is self-contained.
manifest['$schema'] = '../../../../../schemas/v1/rule.json'
json.dump(manifest, sys.stdout, ensure_ascii=False, indent=2)
sys.stdout.write('\n')
PY
  echo "    ${rule_name}: ${hash}"
}

copy_manifest "no-todo"          "tsuzulint_rule_no_todo"
copy_manifest "sentence-length"  "tsuzulint_rule_sentence_length"
copy_manifest "no-doubled-joshi" "tsuzulint_rule_no_doubled_joshi"

echo "==> Generating corpus"
"${SCRIPT_DIR}/gen-corpus.sh"

install_npm_harness() {
  local label="$1"
  local dir="$2"
  if [[ ! -d "${dir}/node_modules" ]]; then
    echo "==> Installing ${label} dependencies (first run)"
    (cd "${dir}" && npm install --no-audit --no-fund --loglevel=error)
  else
    echo "==> ${label} already installed (skip)"
  fi
}

if command -v npm >/dev/null 2>&1; then
  install_npm_harness "textlint" "${BENCHES_DIR}/configs/textlint"
  install_npm_harness "textlint-preset" "${BENCHES_DIR}/configs/textlint-preset"
else
  echo "WARNING: npm not found; skipping textlint installation."
fi

echo "==> Done"
