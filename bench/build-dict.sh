#!/usr/bin/env bash
# Regenerate the IPADIC dictionary container that the benchmark's tzlint config consumes.
#
# The output `ipadic.dict.zst` is gitignored (~13 MiB) — every contributor rebuilds it locally
# from lindera's embedded IPADIC, so the repo carries no large binary. The printed BLAKE3 pin must
# equal `morphology.pin` in `bench/.tzlintrc.json`; if a lindera-dictionary bump changes the
# packed bytes, the pin moves and that config value must be updated to match.
#
# Requires: cargo, the `zstd` CLI, and `b3sum` (e.g. `brew install b3sum` / `cargo install b3sum`).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> packing lindera embedded IPADIC into a container (this compiles the dictionary; slow once)"
( cd "$root" && cargo run --release -p tzlint_morphology_native \
    --example pack_ipadic --features embed-ipadic -- "$tmp/ipadic.dict" )

echo "==> compressing (zstd -19)"
zstd -q -19 -f "$tmp/ipadic.dict" -o "$here/ipadic.dict.zst"

pin=$(b3sum "$here/ipadic.dict.zst" | awk '{print $1}')
echo
echo "wrote $here/ipadic.dict.zst ($(wc -c < "$here/ipadic.dict.zst") bytes)"
echo "pin (BLAKE3 over the compressed container): $pin"
echo
echo "Ensure bench/.tzlintrc.json -> morphology.pin matches the pin above."
