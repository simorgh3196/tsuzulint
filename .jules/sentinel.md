## 2024-07-08 - SSRF bypass via unhandled IPv6 special prefixes
**Vulnerability:** The SSRF blocklist for IPv6 in `validate_dictionary_url` did not block documentation (`2001:db8::/32`), benchmarking (`2001:2::/48`), and discard-only (`100::/64`) prefix ranges, which could theoretically be routed internally in specific configurations.
**Learning:** Rust's `Ipv6Addr::is_documentation()` is currently unstable (issue #27709), so manual segment checks are necessary. Furthermore, the number of segment checks must match the CIDR prefix length (e.g., `/48` needs exactly three 16-bit segment checks).
**Prevention:** Ensure all special-purpose IP ranges are manually checked for SSRF blocklists using `addr.segments()` when standard library helpers are unstable.
