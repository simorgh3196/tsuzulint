## 2025-05-29 - Centralized Bounded I/O
**Vulnerability:** The lack of a centralized boundary utility could lead to TOCTOU and memory exhaustion vulnerabilities from unbounded file reads.
**Learning:** Raw I/O APIs should be disabled codebase-wide using clippy `disallowed-methods` to force developers to go through a single, heavily scrutinized `tzlint_core::io` wrapper.
**Prevention:** Using `Read::take(limit + 1)` prevents memory exhaustion and completely sidesteps TOCTOU file size checks by reading blindly up to the cap.
