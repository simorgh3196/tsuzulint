# TsuzuLint developer tasks. CI is the trust boundary; these mirror it locally.
# `just <task>`; `just` lists tasks.

default:
    @just --list

build:
    cargo build --workspace

# Tests run via nextest; doctests run separately (nextest does not execute doctests).
test:
    cargo nextest run --workspace
    cargo test --workspace --doc

# Just the doctests.
test-doc:
    cargo test --workspace --doc

# The native lindera backend's mapping tests against embedded IPADIC (the dictionary is compiled
# into the test binary by `embed-ipadic`; no network at test time). Default-off, so the normal
# `test` run skips the heavy embedded-dictionary build; CI runs this in a dedicated linux job.
test-native:
    cargo nextest run -p tzlint_morphology_native --features embed-ipadic

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# wasm32 build of the morphology-seam crates (model + injected-provider seam + engine), no native
# backend — mirrors CI. Both wasm targets; run `rustup target add wasm32-unknown-unknown
# wasm32-wasip1` once if missing.
wasm:
    cargo build -p tzlint_ast -p tzlint_pdk -p tzlint_core --target wasm32-unknown-unknown
    cargo build -p tzlint_ast -p tzlint_pdk -p tzlint_core --target wasm32-wasip1

bench:
    cargo bench --workspace

# Coverage (requires cargo-llvm-cov: `cargo install cargo-llvm-cov`).
# HTML report -> target/llvm-cov/html/index.html. Measured over the nextest-run tests;
# doctest line coverage needs nightly (`--doctests`) and is omitted on stable.
coverage:
    cargo llvm-cov nextest --workspace --html

# lcov output for CI / Codecov.
coverage-lcov:
    cargo llvm-cov nextest --workspace --lcov --output-path lcov.info

# CI-equivalent gate.
check: lint test
