## 2024-05-28 - Fast UTF-8 character counting

**Learning:** When needing to just count characters in a UTF-8 string and when `slice.chars().count()` is the bottleneck, we can take advantage of the UTF-8 encoding specification. A byte that is not a continuation byte (`0x80 <= b <= 0xBF`) starts a new character. In Rust, converting a byte `b` to an `i8`, this condition becomes `(b as i8) >= -0x40`. Iterating over `as_bytes()` and checking this condition provides significant speedup over standard `chars().count()` decoding. Also, replacing `binary_search` with `partition_point` avoids branching overhead when finding the insertion index.

**Action:** Look for places where `chars().count()` is used purely to count characters without needing the decoded value, and manually iterate over the bytes checking for non-continuation bytes using `(b as i8) >= -0x40`. Replace `binary_search` matches with `partition_point` when we need an insertion/less-than-or-equal index directly.
