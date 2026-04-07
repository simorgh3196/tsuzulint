I need to fix two issues:
1. `cargo fmt` failed in `crates/tsuzulint_registry/src/resolver.rs` (needs to run `cargo fmt --all`).
2. `mknodat` is not available in `rustix::fs` on macOS.

If `mknodat` is not on macOS in `rustix`, I can conditionally compile the test for Linux only using `#[cfg(target_os = "linux")]` instead of `#[cfg(unix)]`. Let's test this locally.
