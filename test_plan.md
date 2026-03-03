1. **Identify Vulnerability:**
   - The `tsuzulint_cache` crate reads the entire cache file (`cache.rkyv`) into memory using `fs::read(&cache_file)` without any size limit.
   - This can lead to an Out-of-Memory (OOM) Denial of Service vulnerability if the cache file is maliciously crafted or becomes excessively large.
2. **Implement Fix:**
   - Define a `MAX_CACHE_SIZE` constant in `crates/tsuzulint_cache/src/manager.rs` (e.g., 100MB).
   - Check `fs::metadata(&cache_file)?.len()` before reading the content into memory.
   - If the size exceeds `MAX_CACHE_SIZE`, log a warning and return `Ok(())` (treat it as an empty/invalid cache rather than failing hard).
3. **Pre-commit Steps:**
   - Run tests and formatting to ensure code correctness and consistency.
4. **Submit:**
   - Create a PR with the required format for Sentinel.
