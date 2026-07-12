## 2024-07-12 - Missing Unstable IPv6 Prefix Protection in SSRF Blocklist
**Vulnerability:** The SSRF blocklist failed to block the documentation (`2001:db8::/32`) and benchmarking (`2001:2::/48`) prefix ranges for IPv6, relying on standard Rust `Ipv6Addr` which has unstable helpers for these ranges.
**Learning:** Always manually validate specific IPv6 prefix ranges (e.g., checking `segments`) when building security boundaries, as standard library features might be incomplete or unstable.
**Prevention:** Ensure explicit prefix range checks for IPv6 addresses that shouldn't be routable or fetched to avoid bypasses, specifically checking documentation, benchmark, and discard prefixes if necessary.
