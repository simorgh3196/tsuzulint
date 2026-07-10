## 2024-05-15 - Unstable IPv6 Helpers & SSRF Blocklist
**Vulnerability:** Insufficient SSRF protection for IPv6 special-purpose ranges (Documentation, Benchmarking, Discard-only).
**Learning:** Ipv6Addr helper methods like is_documentation() are currently unstable (issue #27709).
**Prevention:** Manually validate the address segments mapping CIDR prefix length to the number of 16-bit segments (e.g., segments[0] == 0x2001 && segments[1] == 0x0db8 for 2001:db8::/32).
