//! The CLI's network-capable [`Host`]: the real filesystem plus an HTTPS dictionary fetch.
//!
//! [`tzlint_core`]'s own [`NativeHost`] is deliberately network-free so the core carries no
//! networking dependency and keeps building for `wasm32`. The native `tzlint` binary, by contrast,
//! must be able to *acquire* a morphology dictionary, so it runs on [`CliHost`]: every filesystem
//! method delegates to [`NativeHost`], and [`fetch`](Host::fetch) is implemented here with `ureq`
//! (rustls TLS). Keeping the HTTP client in the binary — not the library — is what lets the core
//! stay wasm-clean.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use tzlint_core::io::{DirEntry, Host, IoError, NativeHost};

/// A [`Host`] for the native CLI: [`NativeHost`]'s filesystem access plus a real HTTPS
/// [`fetch`](Host::fetch) over `ureq` + rustls.
#[derive(Debug, Default, Clone, Copy)]
pub struct CliHost {
    fs: NativeHost,
}

impl CliHost {
    /// Create a CLI host (stateless; the underlying [`NativeHost`] is a unit type).
    pub fn new() -> Self {
        CliHost { fs: NativeHost }
    }
}

impl Host for CliHost {
    fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
        self.fs.read_to_string(path, limit)
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
        self.fs.write_atomic(path, contents)
    }

    fn exists(&self, path: &Path) -> bool {
        self.fs.exists(path)
    }

    fn list_dir(&self, dir: &Path) -> Result<Vec<DirEntry>, IoError> {
        self.fs.list_dir(dir)
    }

    fn read_bytes(&self, path: &Path, limit: usize) -> Result<Vec<u8>, IoError> {
        self.fs.read_bytes(path, limit)
    }

    fn create_dir_all(&self, dir: &Path) -> Result<(), IoError> {
        self.fs.create_dir_all(dir)
    }

    /// Fetch `url` over HTTPS, capping the response at `limit` bytes.
    ///
    /// `url` is expected to have already passed `tzlint_core::net::validate_dictionary_url`. The
    /// client adds defense in depth on top of that guard:
    /// - **`https_only`** — the client itself refuses any non-`https` request.
    /// - **`max_redirects(0)`** — redirects are *not* followed. A redirect target is not re-checked
    ///   by the SSRF guard, so following a `3xx` to an internal address would bypass it; operators
    ///   pin the final URL instead.
    /// - **per-phase timeouts** — DNS resolve, TCP+TLS connect, response-header receipt, and body
    ///   receipt are each bounded, so a server that stalls at *any* phase (including one that
    ///   accepts the connection but never sends headers) cannot hang provisioning.
    ///
    /// The body is read through a streaming handle with the same one-past-`limit` cap the rest of
    /// the [`Host`] boundary uses, so an over-size response is reported as [`IoError::TooLarge`]
    /// rather than exhausting memory. A fresh agent per call is fine: provisioning is a rare,
    /// one-shot setup step, not a hot path.
    ///
    /// Not yet covered (a documented follow-up): re-validating the *resolved* address to defend
    /// against DNS rebinding (a hostname that resolves to an internal IP). The pinned-hash check in
    /// `tzlint_core::dict` is the backstop — a fetch of the wrong endpoint cannot match the pin.
    fn fetch(&self, url: &str, limit: usize) -> Result<Vec<u8>, IoError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .timeout_resolve(Some(Duration::from_secs(30)))
            .timeout_connect(Some(Duration::from_secs(30)))
            // `timeout_recv_body` only bounds the *body* phase; header receipt needs its own bound
            // (`timeout_recv_response`) or `call()` could wait forever for a connected-but-silent peer.
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(300)))
            .build()
            .into();
        let mut response = agent
            .get(url)
            .call()
            .map_err(|e| IoError::Other(format!("dictionary fetch failed: {e}")))?;
        // `as_reader()` is itself unbounded; the size cap lives in `read_to_limit` so it is
        // unit-testable without a network.
        read_to_limit(response.body_mut().as_reader(), limit)
    }
}

/// Read `reader` fully into a `Vec`, rejecting more than `limit` bytes.
///
/// Uses the same one-past-`limit` cap as the rest of the [`Host`] boundary (`Host::read_bytes`):
/// reading `limit + 1` lets "exactly at the limit" be distinguished from "over it", which is
/// reported as [`IoError::TooLarge`] rather than buffering an unbounded response. Factored out of
/// [`CliHost::fetch`] so the cap logic is testable with an in-memory reader (the `ureq` transport
/// itself cannot be exercised offline).
fn read_to_limit(reader: impl Read, limit: usize) -> Result<Vec<u8>, IoError> {
    let mut bytes = Vec::new();
    reader
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| IoError::Other(format!("reading dictionary response failed: {e}")))?;
    if bytes.len() > limit {
        return Err(IoError::TooLarge { limit });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tzlint_core::io::MAX_DICT;

    #[test]
    fn delegates_filesystem_operations_to_the_native_host() {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};

        let r1 = RandomState::new().build_hasher().finish();
        let r2 = RandomState::new().build_hasher().finish();

        // `CliHost` must be a drop-in for `NativeHost` on every filesystem method (the CLI runs
        // entirely on it), so a write/read round-trip through `CliHost` behaves identically.
        let dir = std::env::temp_dir().join(format!(
            "tzlint-clihost-{}-{:016x}-{:016x}",
            std::process::id(),
            r1,
            r2
        ));
        let host = CliHost::new();
        host.create_dir_all(&dir).unwrap();
        let path = dir.join("d.bin");
        let blob: &[u8] = &[0xFF, 0x00, 0xFE, b'x'];
        host.write_atomic(&path, blob).unwrap();
        assert!(host.exists(&path));
        assert_eq!(host.read_bytes(&path, MAX_DICT).unwrap(), blob);
        assert!(
            host.list_dir(&dir)
                .unwrap()
                .iter()
                .any(|e| e.name == "d.bin")
        );
        // A UTF-8 file also round-trips through the string read (exercises that delegation too).
        let text_path = dir.join("note.md");
        host.write_atomic(&text_path, "見出し\n".as_bytes())
            .unwrap();
        assert_eq!(
            host.read_to_string(&text_path, MAX_DICT).unwrap(),
            "見出し\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_to_limit_caps_at_one_past_the_limit() {
        use std::io::Cursor;
        // Under the limit: returned verbatim.
        assert_eq!(
            read_to_limit(Cursor::new(b"abc".to_vec()), 10).unwrap(),
            b"abc"
        );
        // Exactly at the limit: accepted (the off-by-one boundary, mirroring `read_bytes`).
        assert_eq!(
            read_to_limit(Cursor::new(b"0123456789".to_vec()), 10).unwrap(),
            b"0123456789"
        );
        // One over the limit: rejected as TooLarge rather than buffered.
        assert!(matches!(
            read_to_limit(Cursor::new(b"0123456789".to_vec()), 9),
            Err(IoError::TooLarge { limit: 9 })
        ));
    }

    #[test]
    fn fetch_refuses_a_non_https_url_without_touching_the_network() {
        // `https_only` makes the client reject a cleartext URL up front (no connection attempt),
        // so this is deterministic and offline. It surfaces as an `Other` I/O error, never a panic.
        let host = CliHost::new();
        let err = host
            .fetch("http://dict.example.com/d.zst", MAX_DICT)
            .unwrap_err();
        assert!(matches!(err, IoError::Other(_)), "got {err:?}");
    }
}
