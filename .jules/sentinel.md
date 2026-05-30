## 2025-05-29 - Centralized Bounded I/O
**Vulnerability:** The lack of a centralized boundary utility could lead to TOCTOU and memory exhaustion vulnerabilities from unbounded file reads.
**Learning:** Raw I/O APIs should be disabled codebase-wide using clippy `disallowed-methods` to force developers to go through a single, heavily scrutinized `tzlint_core::io` wrapper.
**Prevention:** Using `Read::take(limit + 1)` prevents memory exhaustion and completely sidesteps TOCTOU file size checks by reading blindly up to the cap.
## 2025-05-29 - Prevent read limit overflow
**Vulnerability:** Adding 1 to a user-provided limit can cause an integer overflow leading to a 0 size limit which avoids the check entirely.
**Learning:** Always use `checked_add` when adding to untrusted input limits to prevent integer overflow bypass.
**Prevention:** Handled the overflow condition using `limit.checked_add(1)` and returning an `InvalidInput` error.
