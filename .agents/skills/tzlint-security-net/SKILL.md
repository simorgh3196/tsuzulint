---
name: tzlint-security-net
description: Two-layer SSRF model over IPv4+IPv6, transition-form unwrapping, scoped tokens, dialed-IP validation.
---

**Rule (registry/network paths).**
- **Two layers:** a configurable host trust list (default `github.com` only) **plus**
  always-on dangerous-target blocking over **both IPv4 and IPv6** after normalization:
  loopback, `0.0.0.0`/`::`, link-local (`169.254.0.0/16`, `fe80::/10`, incl. the
  `169.254.169.254` metadata address), private (`10/8`,`172.16/12`,`192.168/16`,
  `100.64/10`, `fc00::/7`). **Unwrap** IPv4-mapped/compat/NAT64/6to4 to the embedded IPv4
  and re-check.
- `allow_private_targets` relaxes **only** RFC1918/ULA/CGNAT — never link-local, metadata,
  loopback, or unspecified. The hard-block list is evaluated last and is non-overridable.
- **DNS rebinding:** resolve all A/AAAA, validate every candidate (and IP-literal hosts),
  dial exactly the validated/pinned `SocketAddr`, re-resolve+re-validate on every redirect
  hop. SNI/Host uses the hostname; the socket uses the pinned IP.
- **Tokens:** attach only to an exact origin (scheme+host+port); strip `Authorization` on
  any scheme/host/port change; never send over a non-HTTPS hop; never log or cache tokens.

**Why.** SSRF and credential-leak bugs recur exactly where validation is partial
(IPv4-only, resolver-output-only, host-only token scoping).

**How to apply.** This module is migrated intact with its tests — extend, don't rewrite.
Keep the NAT64 / IPv4-mapped / IPv4-compat / 6to4 / dual-stack test vectors green.
