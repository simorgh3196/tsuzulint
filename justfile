# TsuzuLint developer tasks. CI is the trust boundary; these mirror it locally.
# `just <task>`; `just` lists tasks.

default:
    @just --list

build:
    cargo build --workspace

test:
    cargo test --workspace

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

wasm:
    cargo build -p tzlint_core --target wasm32-unknown-unknown

bench:
    cargo bench --workspace

# CI-equivalent gate.
check: lint test
