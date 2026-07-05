## 2025-07-05 - Enhanced SSRF protection for IPv6

**Vulnerability:** The `validate_dictionary_url` SSRF protection in `tzlint_core` did not block certain special-purpose IPv6 address ranges defined in RFCs, potentially allowing an attacker to probe or fetch internal resources via documentation (2001:db8::/32), BMWG benchmarking (2001:2::/48), or discard (100::/64) IPv6 prefixes.

**Learning:** It's important to cross-reference IP blocklists with IANA's Special-Purpose Address Registries to ensure comprehensive coverage against SSRF vectors. Even seemingly harmless prefixes like the documentation block could be routed internally depending on the network configuration. Additionally, when implementing subnet checks based on segment arrays (e.g., `segments[0] == 0x2001 && segments[1] == 0x0002`), always ensure the bitmask is correctly mapped (e.g., a `/48` requires checking the first 3 segments: `&& segments[2] == 0x0000`).

**Prevention:** Regularly audit the IP blocklist against updated IANA registries, and write precise unit tests that explicitly test boundary conditions for blocked prefixes to prevent mathematical inaccuracies in the mask checks.
