#!/usr/bin/env bash
# Generate benchmark corpus at multiple scales from a seed document.
#
# Output:
#   benches/corpus/small/       ~5KB total   (1 seed copy, 1 file)
#   benches/corpus/medium/      ~500KB total (10 seed copies, 10 files)
#   benches/corpus/large/       ~5MB total   (100 seed copies, 100 files)
#   benches/corpus/monolithic/  ~1MB total   (single huge file)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHES_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
CORPUS_DIR="${BENCHES_DIR}/corpus"
SEED="${CORPUS_DIR}/seed.md"

if [[ ! -f "${SEED}" ]]; then
  echo "Seed file not found: ${SEED}" >&2
  exit 1
fi

rm -rf "${CORPUS_DIR}/small" "${CORPUS_DIR}/medium" "${CORPUS_DIR}/large" "${CORPUS_DIR}/monolithic"
mkdir -p "${CORPUS_DIR}/small" "${CORPUS_DIR}/medium" "${CORPUS_DIR}/large" "${CORPUS_DIR}/monolithic"

# Small: just copy the seed as a single file.
cp "${SEED}" "${CORPUS_DIR}/small/doc-000.md"

# Medium: 10 files, each ~2x the seed (duplicated content).
for i in $(seq 0 9); do
  padded=$(printf '%03d' "${i}")
  {
    cat "${SEED}"
    echo
    echo "<!-- variant ${padded} -->"
    echo
    cat "${SEED}"
  } > "${CORPUS_DIR}/medium/doc-${padded}.md"
done

# Large: 100 files, each ~2x the seed.
for i in $(seq 0 99); do
  padded=$(printf '%03d' "${i}")
  {
    cat "${SEED}"
    echo
    echo "<!-- variant ${padded} -->"
    echo
    cat "${SEED}"
  } > "${CORPUS_DIR}/large/doc-${padded}.md"
done

# Monolithic: single 1MB-ish file to test per-file overhead baseline.
: > "${CORPUS_DIR}/monolithic/big.md"
for _ in $(seq 1 250); do
  cat "${SEED}" >> "${CORPUS_DIR}/monolithic/big.md"
  echo >> "${CORPUS_DIR}/monolithic/big.md"
done

echo "Corpus generated:"
du -sh "${CORPUS_DIR}/small" "${CORPUS_DIR}/medium" "${CORPUS_DIR}/large" "${CORPUS_DIR}/monolithic"
printf '  small:      %s files\n' "$(find "${CORPUS_DIR}/small" -type f | wc -l | tr -d ' ')"
printf '  medium:     %s files\n' "$(find "${CORPUS_DIR}/medium" -type f | wc -l | tr -d ' ')"
printf '  large:      %s files\n' "$(find "${CORPUS_DIR}/large" -type f | wc -l | tr -d ' ')"
printf '  monolithic: %s files\n' "$(find "${CORPUS_DIR}/monolithic" -type f | wc -l | tr -d ' ')"
