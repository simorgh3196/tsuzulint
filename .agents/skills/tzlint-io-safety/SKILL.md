---
name: tzlint-io-safety
description: All boundary I/O goes through the centralized tzlint_core::io module; no raw fs/network elsewhere.
---

**Rule.** Every file/network read or write goes through `tzlint_core::io`
(`read_with_limit`, the atomic-write helper). Raw `std::fs::read*`/`write`, `libc`, and
ad-hoc network calls are forbidden outside that module (enforced by clippy
`disallowed-methods` + a CI `io-guard` grep). Reads use `Read::take(max + 1)` (no
metadata-then-read; avoids TOCTOU). Durable writes are tmp (`O_CREAT|O_EXCL`, 0600, same
dir) + `fsync(tmp)` + rename + `fsync(parent dir)` — used by **both** the cache and
`--fix`. FIFO/`O_NONBLOCK` via `rustix`, never `unsafe libc`.

**Why.** A single I/O mistake repeated across many entry points is the most frequent
recurring bug/vuln class; one wrapper makes it structurally impossible.

**How to apply.** Need to touch a file or the network? Call (or extend) `tzlint_core::io`.
The wrapper functions are the only place allowed to `#[allow(clippy::disallowed_methods)]`,
with a justifying comment. Surface failures as `IoError`/`SecurityError`.
