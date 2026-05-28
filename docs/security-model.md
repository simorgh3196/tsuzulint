# Security model

> Status: template (M0/M6). See the `tzlint-io-safety` and `tzlint-security-net` skills.

- **Centralized I/O:** one `read_with_limit` + one atomic-write helper; raw fs/network
  forbidden elsewhere (clippy + CI grep). `Read::take(max+1)`; no metadata-then-read.
- **Atomic writes:** tmp (`O_CREAT|O_EXCL`, 0600) + fsync + rename + parent-dir fsync;
  used by cache and `--fix`.
- **SSRF (two-layer):** trust list + always-on dangerous-target blocking over IPv4 **and
  IPv6** (loopback / unspecified / link-local incl. metadata / private), transition-form
  unwrapping, dialed-IP validation, per-hop re-validation. `allow_private_targets` never
  relaxes metadata/link-local/loopback.
- **Tokens:** scoped to scheme+host+port; stripped on any origin change; never on non-HTTPS;
  never logged/cached.
- **WASM sandbox:** native (wasmtime fuel/epoch/memory/WASI) vs browser (JS sandbox, no
  fuel; shared-memory AST-integrity caveat — first-party/bundled plugins for v1).
