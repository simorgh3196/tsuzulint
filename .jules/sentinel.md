## 2024-07-13 - SSRF Bypass via Incomplete IPv6 Blocklist
**Vulnerability:** The IPv6 URL blocklist for dictionary fetching missed the documentation (`2001:db8::/32`), benchmarking (`2001:2::/48`), and discard-only (`100::/64`) prefixes, potentially allowing bypasses.
**Learning:** Rust's `Ipv6Addr::is_documentation()` is unstable (issue #27709), so we must manually check prefix segments.
**Prevention:** Explicitly validate segment ranges for blocked prefixes when standard library methods are unstable.
