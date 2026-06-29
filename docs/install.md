# Installing TsuzuLint

TsuzuLint is the single `tzlint` binary. Pick whichever install method fits your environment;
all of them give you the same command.

> Prebuilt binaries, the npm wrapper, and the editor extension are published **from the 0.1.0
> release onward**. Until then, install from source.

## From source (works today)

With a [Rust toolchain](https://rustup.rs/) (MSRV **1.94**):

```sh
# Install the latest from git (the binary lands in ~/.cargo/bin):
cargo install --git https://github.com/simorgh3196/tsuzulint tzlint_cli

# …or build a local checkout:
git clone https://github.com/simorgh3196/tsuzulint
cd tsuzulint
cargo build --release          # produces target/release/tzlint
```

Once 0.1.0 is published to crates.io, `cargo install tzlint_cli` will work without `--git`.

## Prebuilt binaries (0.1.0 onward)

Each [GitHub Release](https://github.com/simorgh3196/tsuzulint/releases) attaches a
self-contained `tzlint` archive per platform:

| Platform | Asset |
| --- | --- |
| Linux x86_64 | `tzlint-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `tzlint-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS (Apple Silicon) | `tzlint-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `tzlint-<version>-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `tzlint-<version>-x86_64-pc-windows-msvc.zip` |

Download, verify against `SHA256SUMS`, extract, and put `tzlint` on your `PATH`:

```sh
tar xzf tzlint-*-x86_64-unknown-linux-gnu.tar.gz
sudo mv tzlint /usr/local/bin/
```

## npm (0.1.0 onward)

For Node-based projects, a thin wrapper downloads the matching prebuilt binary on install:

```sh
npm install --save-dev tzlint     # or: npx tzlint lint docs/
```

The wrapper just fetches the release binary for your platform and execs it — there is no
JavaScript reimplementation.

## Editor extension

A CLI-backed VSCode extension is in progress; it will be published to the Marketplace alongside
the editor MVP. A full LSP server is the 0.2.0 upgrade. Until then, run `tzlint` from the
terminal or your build pipeline.

## Verify

```sh
tzlint --version
printf 'これはﾊﾛｰという文です。\n' | tzlint lint -
```

## Optional: morphology dictionary

The morphology-backed Japanese rules (e.g. `no-doubled-joshi`) stay inert until you point the
config at a hash-pinned dictionary. This is never bundled in the binary — see
[`docs/morphology.md`](morphology.md) and the
[`morphology` config key](config-reference.md#morphology--dictionary-for-morphology-dependent-rules).
