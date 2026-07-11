## 2025-05-18 - SSRF bypass via IPv6 benchmarking/documentation blocks
**Vulnerability:** The SSRF protection in `net::ipv6_is_blocked` failed to block the IPv6 documentation (`2001:db8::/32`) and benchmarking (`2001:2::/48`) prefix blocks.
**Learning:** These blocks can be routed in internal/private networks or used as unallocated space, leading to SSRF if they are re-assigned or point to internal machines or proxies by configuration. The standard `Ipv6Addr::is_documentation()` is unstable in Rust, so manual segment checking is necessary.
**Prevention:** Explicitly match segments against known documentation (`2001:db8::/32`) and benchmarking (`2001:2::/48`) prefixes when validating IPv6 literals.
