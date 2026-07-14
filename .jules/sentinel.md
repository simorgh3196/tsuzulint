## 2024-07-14 - SSRF bypass via unblocked IPv6 prefixes
**Vulnerability:** The SSRF blocklist in `crates/tzlint_core/src/net.rs` lacked protection against IPv6 documentation (`2001:db8::/32`) and benchmarking (`2001:2::/48`) prefixes, allowing potential internal probing.
**Learning:** Using `Ipv6Addr::segments()` for manual prefix matching is required when standard unstable helpers are unavailable. Prefix lengths explicitly determine the number of 16-bit segments matched (e.g., `/48` needs exactly three segment matches).
**Prevention:** Always manually validate exact IPv6 prefix segment definitions in security-critical network fetch validation when built-in methods are unstable.
