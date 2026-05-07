## 2025-02-14 - Optimize HashMap string keys
**Learning:** In Rust, avoiding `HashMap::entry(key.clone())` with heap allocation is crucial for loop performance. Instead, preferring `&str` keys or `.get_mut()` with `.insert()` is optimal and prevents allocation on already existing entries. I optimized `crates/tsuzulint_cli/src/output/text.rs` to use `HashMap<&str, Duration>` instead of `HashMap<String, Duration>`.
**Action:** Applied patch to text.rs and verified functionality.
