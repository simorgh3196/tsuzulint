# tzlint (npm wrapper)

A thin npm wrapper for [**TsuzuLint**](https://github.com/simorgh3196/tsuzulint) — a fast,
embeddable Japanese `textlint` replacement written in Rust.

This package ships **no JavaScript linter**. On install it downloads the prebuilt native
`tzlint` binary matching your platform from the matching GitHub Release, verifies it against the
release `SHA256SUMS`, and the `tzlint` command execs that binary directly.

## Install

```sh
npm install --save-dev tzlint
# or run ad hoc:
npx tzlint lint docs/
```

```sh
tzlint lint README.md docs/
tzlint fix docs/
tzlint rules list
```

See the [main README](https://github.com/simorgh3196/tsuzulint#usage) and
[`docs/`](https://github.com/simorgh3196/tsuzulint/tree/main/docs) for usage and configuration.

## How it works

- `postinstall` runs [`install.js`](install.js): it resolves your `platform`-`arch` to a release
  asset (`tzlint-<version>-<target>.{tar.gz,zip}`), downloads it, checks its SHA-256 against the
  release `SHA256SUMS`, extracts it, and stores it as `bin/tzlint-bin`.
- The `tzlint` bin entry ([`bin/tzlint.js`](bin/tzlint.js)) is a launcher that forwards argv,
  stdio, and the exit code to that native binary.

Supported targets: Linux (x64, arm64), macOS (x64, arm64), Windows (x64).

Set `TZLINT_SKIP_DOWNLOAD=1` to skip the download (e.g. when building from a source checkout). If
no prebuilt binary exists for your platform, [build from source](https://github.com/simorgh3196/tsuzulint/blob/main/docs/install.md).

> Published from the 0.1.0 release onward. The package version tracks the `tzlint` release it
> installs.
