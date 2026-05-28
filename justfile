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

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

wasm:
    cargo build -p tzlint_core --target wasm32-unknown-unknown

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
