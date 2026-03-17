## 2024-03-17 - Removed redundant string clone during HashMap insertions
**Learning:** In Rust, when moving an owned value (like a `String`) into a `HashMap`, cloning it beforehand is an unnecessary allocation, and moving ownership instead prevents heap allocations.
**Action:** When inserting owned keys or values into structures like `HashMap`, ensure the value is moved rather than cloned unless the original value needs to be retained.
