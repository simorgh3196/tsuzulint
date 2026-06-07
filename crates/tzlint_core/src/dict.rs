//! Morphology-dictionary provisioning: verify a hash-pinned compressed blob, decompress it
//! (zstd, via the pure-Rust `ruzstd` decoder), and cache the result on disk through the [`Host`]
//! boundary.
//!
//! This is the **verify → decompress → cache** half of provisioning. *Acquiring* the compressed
//! blob (a network fetch, with its SSRF guard and an HTTP client) is a separate, host-side
//! concern handled in a later step; here the blob is already present at a local path (a previous
//! download, or an embedder-provided file) and is read through [`Host::read_bytes`].
//!
//! The pinned hash is BLAKE3 over the **compressed** blob and is checked *before* decompression,
//! so a tampered or wrong file is rejected without processing its contents. The **verified
//! compressed blob** is then cached **hash-addressed** (filename = hex of the pinned hash): the
//! cache stores exactly the bytes the pin verifies, so every load — hit or miss — re-checks
//! `blake3(file) == pinned_hash` and decompresses on use. A dictionary upgrade changes the pinned
//! hash, so it is a different cache file and never serves the old one (the cache-invalidation
//! property the dictionary-version tests rely on); an old pre-M2m cache that stored *decompressed*
//! bytes simply fails the pin re-check and is transparently re-provisioned once.

use std::io::Read;
use std::path::Path;

use ruzstd::decoding::StreamingDecoder;

use crate::io::{Host, IoError, MAX_DICT};
use crate::net::{UrlPolicyError, validate_dictionary_url};

/// A failure while provisioning a dictionary.
#[derive(Debug)]
pub enum DictError {
    /// The fetch URL failed the SSRF safety guard (see [`crate::net`]).
    InvalidUrl(UrlPolicyError),
    /// The compressed blob's BLAKE3 hash did not equal the pinned value (wrong or tampered file).
    HashMismatch,
    /// The blob is not valid zstd / could not be decompressed.
    Decompress(String),
    /// The decompressed dictionary exceeded the `limit` bytes.
    TooLarge {
        /// The cap that was exceeded.
        limit: usize,
    },
    /// An underlying boundary-I/O failure (reading the blob, creating the cache dir, writing it).
    Io(IoError),
}

impl core::fmt::Display for DictError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DictError::InvalidUrl(error) => write!(f, "{error}"),
            DictError::HashMismatch => {
                write!(f, "dictionary hash does not match the pinned value")
            }
            DictError::Decompress(reason) => {
                write!(f, "dictionary decompression failed: {reason}")
            }
            DictError::TooLarge { limit } => {
                write!(f, "decompressed dictionary exceeds the {limit}-byte limit")
            }
            DictError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DictError::Io(error) => Some(error),
            DictError::InvalidUrl(error) => Some(error),
            _ => None,
        }
    }
}

impl From<IoError> for DictError {
    fn from(error: IoError) -> Self {
        DictError::Io(error)
    }
}

impl From<UrlPolicyError> for DictError {
    fn from(error: UrlPolicyError) -> Self {
        DictError::InvalidUrl(error)
    }
}

/// The hash-addressed cache filename for a dictionary pinned to `hash`: lowercase hex + `.dict`.
fn cache_file_name(hash: &[u8; 32]) -> String {
    let mut name = String::with_capacity(64 + ".dict".len());
    for byte in hash {
        // `from_digit` is infallible for radix 16 and a nibble (0..=15); the fallback is
        // unreachable and keeps this panic-free (same idiom as `CacheKey::to_hex`).
        name.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        name.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    name.push_str(".dict");
    name
}

/// Provision the dictionary pinned to `pinned_hash`, returning its decompressed bytes.
///
/// Returns the dictionary **in memory** (owned bytes), the one representation every host can use —
/// a browser/wasm embedder has no filesystem path to memory-map. A caller that needs the
/// dictionary repeatedly should hold onto the returned bytes: even a cache hit re-reads, re-hashes,
/// and re-decompresses the blob, so this is a provision-once-then-reuse API, not a per-use lookup.
/// (That per-load decompress is a one-time setup cost under that contract; a native backend that
/// would rather mmap the cached file can grow a path-returning variant later, when such a consumer
/// actually lands — there is none yet.)
///
/// **Cache hit:** [`try_cache_hit`] reads the cached **compressed** blob, re-checks
/// `blake3(file) == pinned_hash`, and decompresses it on use. Because the cache is content-addressed
/// by the pin, a verified hit returns offline without touching `compressed_source`. A hit whose
/// bytes no longer match the pin (bit-rot, truncation, a stale pre-M2m decompressed entry) can only
/// be benign corruption — the pin cannot be forged — so it is treated as a miss and **self-healed**
/// by re-provisioning over it. A genuine read fault ([`IoError::TooLarge`]/[`IoError::Other`]) is
/// **surfaced**, never silently re-provisioned, so a flaky disk is not masked.
///
/// **Cache miss:** read the compressed blob from `compressed_source` through the [`Host`], verify
/// `blake3(blob) == pinned_hash` **before** decompressing (never process an unverified blob),
/// zstd-decompress it (bounded by [`MAX_DICT`]) to produce the return value, cache the **verified
/// compressed blob** (creating `cache_dir` and writing atomically), and return the decompressed
/// bytes. A cache-write failure is **surfaced**, not swallowed: provisioning is a setup step, so a
/// non-writable cache directory is an environment problem the operator should fix rather than have
/// masked by silently re-decompressing on every run.
pub fn provision_dictionary(
    host: &dyn Host,
    cache_dir: &Path,
    compressed_source: &Path,
    pinned_hash: &[u8; 32],
) -> Result<Vec<u8>, DictError> {
    let cache_path = cache_dir.join(cache_file_name(pinned_hash));
    if let Some(hit) = try_cache_hit(host, &cache_path, pinned_hash)? {
        return Ok(hit);
    }

    let compressed = host.read_bytes(compressed_source, MAX_DICT)?;
    verify_decompress_and_cache(host, cache_dir, &cache_path, &compressed, pinned_hash)
}

/// Provision the dictionary pinned to `pinned_hash`, **fetching** the compressed blob from `url`
/// when it is not already cached.
///
/// Identical to [`provision_dictionary`] except the cache-miss source is the network rather than a
/// local path: `url` is first run through [`validate_dictionary_url`] (the SSRF guard) and only a
/// URL that passes is handed to [`Host::fetch`]. Everything else is shared — the cache hit (read +
/// pin re-check + decompress, self-healing a corrupt entry via a re-fetch) and the miss tail
/// (verify `blake3 == pin` **before** decompressing, zstd-decompress bounded by [`MAX_DICT`], cache
/// the verified compressed blob, return it in memory). A *verified* cache hit never touches the
/// network at all (so a pinned, already provisioned dictionary works offline).
pub fn provision_dictionary_from_url(
    host: &dyn Host,
    cache_dir: &Path,
    url: &str,
    pinned_hash: &[u8; 32],
) -> Result<Vec<u8>, DictError> {
    let cache_path = cache_dir.join(cache_file_name(pinned_hash));
    if let Some(hit) = try_cache_hit(host, &cache_path, pinned_hash)? {
        return Ok(hit);
    }

    // Guard the URL *before* any network access, then fetch through the single egress boundary.
    let validated = validate_dictionary_url(url)?;
    let compressed = host.fetch(validated.as_str(), MAX_DICT)?;
    verify_decompress_and_cache(host, cache_dir, &cache_path, &compressed, pinned_hash)
}

/// Try to serve the dictionary from the hash-addressed cache at `cache_path`.
///
/// Returns `Ok(Some(bytes))` for a verified hit (the cached compressed blob re-checked against
/// `pinned_hash` and decompressed), or `Ok(None)` to fall through to a re-provisioning **miss**.
/// Two cases become a miss rather than an error:
/// - **vanished** — [`Host::read_bytes`] returns [`IoError::NotFound`]: either a cold cache or a
///   file removed between an `exists()` check and the read. Reading directly (no pre-check) and
///   mapping `NotFound` to a miss closes that TOCTOU window — a benign miss never becomes fatal.
/// - **corrupt** — the bytes no longer hash to `pinned_hash`: in a content-addressed cache only
///   benign corruption (bit-rot/truncation/a stale pre-M2m decompressed entry) can do this, since
///   the pin cannot be forged; self-heal by re-provisioning over it.
///
/// Any *other* read fault ([`IoError::TooLarge`]/[`IoError::Other`] — e.g. EIO, or a present but
/// unreadable file) is **surfaced** as [`DictError::Io`], never silently downgraded to a miss, so a
/// failing disk is loud rather than masked. `NotFound` is matched specifically for exactly this
/// reason. A post-verify decompression failure is a logical impossibility (the blob hashed to the
/// pin, and the pinned blob decompressed cleanly when first cached) but is surfaced if it occurs.
fn try_cache_hit(
    host: &dyn Host,
    cache_path: &Path,
    pinned_hash: &[u8; 32],
) -> Result<Option<Vec<u8>>, DictError> {
    let compressed = match host.read_bytes(cache_path, MAX_DICT) {
        Ok(bytes) => bytes,
        Err(IoError::NotFound) => return Ok(None), // vanished/cold → miss
        Err(other) => return Err(DictError::Io(other)), // genuine fault → surface
    };
    if blake3::hash(&compressed).as_bytes() != pinned_hash {
        return Ok(None); // benign corruption → self-heal as a miss
    }
    Ok(Some(zstd_decompress(&compressed, MAX_DICT)?))
}

/// Verify `compressed` against `pinned_hash`, decompress it (bounded by [`MAX_DICT`]), cache the
/// **verified compressed blob** at `cache_path` (creating `cache_dir`), and return the decompressed
/// bytes.
///
/// The shared **verify → decompress → cache** tail of both provisioning entry points; they differ
/// only in how `compressed` was acquired (a local read vs a network fetch). The cached bytes are the
/// *compressed* blob — the exact bytes the pin hashes — so [`try_cache_hit`] can re-verify a hit
/// against `pinned_hash`. Decompression happens before the cache write, so a verified-but-corrupt
/// (non-zstd) blob errors out without leaving a cache entry behind.
fn verify_decompress_and_cache(
    host: &dyn Host,
    cache_dir: &Path,
    cache_path: &Path,
    compressed: &[u8],
    pinned_hash: &[u8; 32],
) -> Result<Vec<u8>, DictError> {
    // Verify before decompressing: never process the contents of an unverified blob.
    if blake3::hash(compressed).as_bytes() != pinned_hash {
        return Err(DictError::HashMismatch);
    }
    let decompressed = zstd_decompress(compressed, MAX_DICT)?;

    host.create_dir_all(cache_dir)?;
    // Cache the verified COMPRESSED blob, not the decompressed bytes, so the pin re-verifies a hit.
    host.write_atomic(cache_path, compressed)?;
    Ok(decompressed)
}

/// Decompress the zstd `compressed` blob, rejecting output larger than `limit` bytes.
fn zstd_decompress(compressed: &[u8], limit: usize) -> Result<Vec<u8>, DictError> {
    let decoder =
        StreamingDecoder::new(compressed).map_err(|e| DictError::Decompress(e.to_string()))?;
    let mut out = Vec::new();
    // Cap one past the limit so "exactly at the limit" is distinguishable from "over it"
    // (mirrors `Host::read_bytes`); `saturating_add` keeps `usize::MAX` from overflowing.
    decoder
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|e| DictError::Decompress(e.to_string()))?;
    if out.len() > limit {
        return Err(DictError::TooLarge { limit });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::NativeHost;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A known payload, zstd-compressed offline (`zstd -q -c`); decompresses to [`PAYLOAD`].
    const FIXTURE: &[u8] = &[
        0x28, 0xb5, 0x2f, 0xfd, 0x04, 0x58, 0x21, 0x02, 0x00, 0x74, 0x73, 0x75, 0x7a, 0x75, 0x6c,
        0x69, 0x6e, 0x74, 0x20, 0x6d, 0x6f, 0x72, 0x70, 0x68, 0x6f, 0x6c, 0x6f, 0x67, 0x79, 0x20,
        0x64, 0x69, 0x63, 0x74, 0x69, 0x6f, 0x6e, 0x61, 0x72, 0x79, 0x20, 0x66, 0x69, 0x78, 0x74,
        0x75, 0x72, 0x65, 0x20, 0xe2, 0x80, 0x94, 0x20, 0xe5, 0xbd, 0xa2, 0xe6, 0x85, 0x8b, 0xe7,
        0xb4, 0xa0, 0xe8, 0xbe, 0x9e, 0xe6, 0x9b, 0xb8, 0xe3, 0x83, 0x86, 0xe3, 0x82, 0xb9, 0xe3,
        0x83, 0x88, 0x85, 0x9e, 0x8c, 0x61,
    ];
    const PAYLOAD: &str = "tsuzulint morphology dictionary fixture — 形態素辞書テスト";

    /// A *second*, distinct payload zstd-compressed offline (`zstd -q -c`); decompresses to
    /// [`OTHER_PAYLOAD`]. Used to poison the cache with a blob that is **valid zstd** yet whose hash
    /// differs from the pin — so the `blake3 == pin` re-check (not the decompressor) is what must
    /// reject it.
    const OTHER_FIXTURE: &[u8] = &[
        0x28, 0xb5, 0x2f, 0xfd, 0x24, 0x37, 0xb9, 0x01, 0x00, 0x64, 0x69, 0x66, 0x66, 0x65, 0x72,
        0x65, 0x6e, 0x74, 0x20, 0x76, 0x61, 0x6c, 0x69, 0x64, 0x20, 0x70, 0x61, 0x79, 0x6c, 0x6f,
        0x61, 0x64, 0x20, 0xe2, 0x80, 0x94, 0x20, 0xe5, 0x88, 0xa5, 0xe3, 0x81, 0xae, 0xe6, 0x9c,
        0x89, 0xe5, 0x8a, 0xb9, 0xe3, 0x83, 0x9a, 0xe3, 0x82, 0xa4, 0xe3, 0x83, 0xad, 0xe3, 0x83,
        0xbc, 0xe3, 0x83, 0x89, 0xcb, 0xeb, 0x70, 0x82,
    ];
    const OTHER_PAYLOAD: &str = "different valid payload — 別の有効ペイロード";

    /// A unique, freshly-created temp directory (cleaned up by the caller).
    fn temp_dir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tzlint-dict-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        NativeHost.create_dir_all(&dir).unwrap();
        dir
    }

    fn pin_of(blob: &[u8]) -> [u8; 32] {
        *blake3::hash(blob).as_bytes()
    }

    /// A [`Host`] whose filesystem operations are the real [`NativeHost`] (so caching is exercised
    /// against a temp dir) but whose [`fetch`](Host::fetch) returns a canned response — modeling the
    /// CLI's network host without any actual networking. Counts fetches so a test can assert a cache
    /// hit (or a rejected URL) never reached the network.
    struct FetchHost {
        fs: NativeHost,
        response: Vec<u8>,
        fail: bool,
        fetches: std::cell::Cell<u32>,
    }

    impl FetchHost {
        fn serving(response: &[u8]) -> Self {
            FetchHost {
                fs: NativeHost,
                response: response.to_vec(),
                fail: false,
                fetches: std::cell::Cell::new(0),
            }
        }
        fn failing() -> Self {
            FetchHost {
                fs: NativeHost,
                response: Vec::new(),
                fail: true,
                fetches: std::cell::Cell::new(0),
            }
        }
    }

    impl Host for FetchHost {
        fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
            self.fs.read_to_string(path, limit)
        }
        fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
            self.fs.write_atomic(path, contents)
        }
        fn exists(&self, path: &Path) -> bool {
            self.fs.exists(path)
        }
        fn read_bytes(&self, path: &Path, limit: usize) -> Result<Vec<u8>, IoError> {
            self.fs.read_bytes(path, limit)
        }
        fn create_dir_all(&self, dir: &Path) -> Result<(), IoError> {
            self.fs.create_dir_all(dir)
        }
        fn fetch(&self, _url: &str, limit: usize) -> Result<Vec<u8>, IoError> {
            self.fetches.set(self.fetches.get() + 1);
            if self.fail {
                return Err(IoError::Other("network down".to_string()));
            }
            if self.response.len() > limit {
                return Err(IoError::TooLarge { limit });
            }
            Ok(self.response.clone())
        }
    }

    const SOURCE_URL: &str = "https://dict.example.com/ja/ipadic.dict.zst";

    #[test]
    fn provision_decompresses_caches_and_then_serves_from_the_cache() {
        let dir = temp_dir();
        let host = NativeHost;
        let source = dir.join("ipadic.zst");
        host.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);

        // Cache miss: verifies, decompresses, and caches.
        let first = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        assert_eq!(first, PAYLOAD.as_bytes());
        assert!(host.exists(&cache_dir.join(cache_file_name(&pinned))));

        // Remove the source so a re-read would fail; the second call must hit the cache instead.
        std::fs::remove_file(&source).unwrap();
        let second = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        assert_eq!(second, PAYLOAD.as_bytes());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provision_rejects_a_blob_whose_hash_is_not_pinned() {
        let dir = temp_dir();
        let host = NativeHost;
        let source = dir.join("tampered.zst");
        host.write_atomic(&source, FIXTURE).unwrap();
        // A pin that does not match the blob → rejected before any decompression, no cache write.
        let wrong = [0u8; 32];
        let err = provision_dictionary(&host, &dir.join("cache"), &source, &wrong).unwrap_err();
        assert!(matches!(err, DictError::HashMismatch));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provision_errors_on_a_verified_but_non_zstd_blob() {
        let dir = temp_dir();
        let host = NativeHost;
        let garbage: &[u8] = b"this is not a zstd frame";
        let source = dir.join("garbage.zst");
        host.write_atomic(&source, garbage).unwrap();
        // The hash matches (so verification passes) but the bytes are not zstd → Decompress error.
        let pinned = pin_of(garbage);
        let err = provision_dictionary(&host, &dir.join("cache"), &source, &pinned).unwrap_err();
        assert!(matches!(err, DictError::Decompress(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provision_surfaces_a_missing_source_as_an_io_error() {
        let dir = temp_dir();
        let host = NativeHost;
        let missing = dir.join("does-not-exist.zst");
        let err = provision_dictionary(&host, &dir.join("cache"), &missing, &pin_of(FIXTURE))
            .unwrap_err();
        assert!(matches!(err, DictError::Io(IoError::NotFound)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zstd_decompress_rejects_output_larger_than_the_limit() {
        // The fixture decompresses to exactly 68 bytes; a 10-byte cap must reject it as TooLarge.
        let err = zstd_decompress(FIXTURE, 10).unwrap_err();
        assert!(matches!(err, DictError::TooLarge { limit: 10 }));
        // Pin the off-by-one boundary (mirrors io.rs's read-cap tests): exactly-at-limit is
        // accepted, one under is rejected — guards the `> limit` / `take(limit+1)` arithmetic.
        assert_eq!(zstd_decompress(FIXTURE, 68).unwrap(), PAYLOAD.as_bytes());
        assert!(matches!(
            zstd_decompress(FIXTURE, 67).unwrap_err(),
            DictError::TooLarge { limit: 67 }
        ));
        // At a generous cap it decompresses fully.
        assert_eq!(
            zstd_decompress(FIXTURE, MAX_DICT).unwrap(),
            PAYLOAD.as_bytes()
        );
    }

    #[test]
    fn provision_from_url_fetches_verifies_caches_then_serves_offline() {
        let dir = temp_dir();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);
        let host = FetchHost::serving(FIXTURE);

        // Cache miss: the SSRF guard passes, the blob is fetched, verified, decompressed, cached.
        let first = provision_dictionary_from_url(&host, &cache_dir, SOURCE_URL, &pinned).unwrap();
        assert_eq!(first, PAYLOAD.as_bytes());
        assert_eq!(host.fetches.get(), 1);
        assert!(host.exists(&cache_dir.join(cache_file_name(&pinned))));

        // Second call hits the cache and must NOT touch the network (works offline once pinned).
        let second = provision_dictionary_from_url(&host, &cache_dir, SOURCE_URL, &pinned).unwrap();
        assert_eq!(second, PAYLOAD.as_bytes());
        assert_eq!(host.fetches.get(), 1, "a cache hit must not fetch");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provision_from_url_rejects_an_unsafe_url_before_fetching() {
        let dir = temp_dir();
        let host = FetchHost::serving(FIXTURE);
        let pinned = pin_of(FIXTURE);
        // A cleartext (non-https) URL is refused by the guard …
        let err = provision_dictionary_from_url(
            &host,
            &dir.join("cache"),
            "http://dict.example.com/d.zst",
            &pinned,
        )
        .unwrap_err();
        assert!(matches!(err, DictError::InvalidUrl(_)));
        // … as is a loopback host (an SSRF probe).
        let err = provision_dictionary_from_url(
            &host,
            &dir.join("cache"),
            "https://127.0.0.1/d.zst",
            &pinned,
        )
        .unwrap_err();
        assert!(matches!(err, DictError::InvalidUrl(_)));
        assert_eq!(host.fetches.get(), 0, "an unsafe URL must never be fetched");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provision_from_url_rejects_a_fetched_blob_whose_hash_is_not_pinned() {
        let dir = temp_dir();
        let host = FetchHost::serving(FIXTURE);
        // The fetched bytes are FIXTURE, but the pin is wrong → rejected after fetch, before
        // decompression, with nothing cached.
        let wrong = [0u8; 32];
        let err = provision_dictionary_from_url(&host, &dir.join("cache"), SOURCE_URL, &wrong)
            .unwrap_err();
        assert!(matches!(err, DictError::HashMismatch));
        assert_eq!(host.fetches.get(), 1);
        assert!(!host.exists(&dir.join("cache").join(cache_file_name(&wrong))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provision_from_url_propagates_a_fetch_failure_as_an_io_error() {
        let dir = temp_dir();
        let host = FetchHost::failing();
        let err =
            provision_dictionary_from_url(&host, &dir.join("cache"), SOURCE_URL, &pin_of(FIXTURE))
                .unwrap_err();
        assert!(matches!(err, DictError::Io(IoError::Other(_))));
        assert_eq!(host.fetches.get(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dict_error_display_and_source() {
        use crate::net::UrlPolicyError;
        use std::error::Error;
        assert_eq!(
            DictError::HashMismatch.to_string(),
            "dictionary hash does not match the pinned value"
        );
        assert!(
            DictError::Decompress("bad frame".into())
                .to_string()
                .contains("bad frame")
        );
        assert_eq!(
            DictError::TooLarge { limit: 8 }.to_string(),
            "decompressed dictionary exceeds the 8-byte limit"
        );
        let io = DictError::Io(IoError::NotFound);
        assert_eq!(io.to_string(), "file not found");
        assert!(io.source().is_some()); // the Io variant chains its source
        assert!(DictError::HashMismatch.source().is_none());
        // The InvalidUrl variant forwards the guard's message and chains its source.
        let bad_url = DictError::InvalidUrl(UrlPolicyError::NotHttps);
        assert_eq!(bad_url.to_string(), "dictionary URL must use https");
        assert!(bad_url.source().is_some());
    }

    /// A path is a dictionary *cache* entry iff its name ends in `.dict` (see [`cache_file_name`]);
    /// the source blob is a `.zst`. The fault-injecting mock hosts below key their behavior off
    /// this so they disturb only the cache read, never the source read.
    fn is_cache_entry(path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "dict")
    }

    /// The fault a [`CacheFaultHost`] injects when the *cache* entry is read.
    enum CacheFault {
        /// The entry is gone by read time ([`IoError::NotFound`]) — models a cold cache or a file
        /// removed between an `exists()` hint and the read (the TOCTOU race). Must become a miss.
        Vanished,
        /// A genuine read fault ([`IoError::Other`]) — must be **surfaced**, never downgraded to a
        /// miss (only `NotFound` self-heals).
        Unreadable,
        /// An over-[`MAX_DICT`] entry ([`IoError::TooLarge`]) — like `Unreadable`, a genuine fault
        /// that must be **surfaced**, never treated as a benign miss.
        Oversize,
    }

    /// A host that injects a [`CacheFault`] on the *cache* read while source reads/writes stay the
    /// real [`NativeHost`], so provisioning can still recover. Source-vs-cache is keyed off
    /// [`is_cache_entry`], so only the cache read is disturbed.
    ///
    /// `exists()` answers `true` for the cache entry: the fault models a file that is *present then
    /// gone/faulty by read time*, not a cold miss. That also makes these hosts discriminate a
    /// regression that re-introduces an `exists()` pre-check — such a mutant would take the read
    /// branch (and surface the injected fault) where the current pre-check-free code does not.
    struct CacheFaultHost {
        fs: NativeHost,
        fault: CacheFault,
    }

    impl Host for CacheFaultHost {
        fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
            self.fs.read_to_string(path, limit)
        }
        fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
            self.fs.write_atomic(path, contents)
        }
        fn exists(&self, path: &Path) -> bool {
            is_cache_entry(path) || self.fs.exists(path)
        }
        fn read_bytes(&self, path: &Path, limit: usize) -> Result<Vec<u8>, IoError> {
            if is_cache_entry(path) {
                return Err(match self.fault {
                    CacheFault::Vanished => IoError::NotFound,
                    CacheFault::Unreadable => {
                        IoError::Other("simulated disk read error".to_string())
                    }
                    CacheFault::Oversize => IoError::TooLarge { limit },
                });
            }
            self.fs.read_bytes(path, limit)
        }
        fn create_dir_all(&self, dir: &Path) -> Result<(), IoError> {
            self.fs.create_dir_all(dir)
        }
    }

    #[test]
    fn cache_stores_the_verified_compressed_blob_not_the_decompressed_bytes() {
        let dir = temp_dir();
        let host = NativeHost;
        let source = dir.join("ipadic.zst");
        host.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);

        let out = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        assert_eq!(out, PAYLOAD.as_bytes());
        // The cache holds the COMPRESSED blob — the exact bytes `pinned_hash` verifies — so a hit
        // can re-check `blake3(file) == pin`. (A decompressed cache could not be pin-verified.)
        // Read the cache file through the host boundary (raw filesystem reads are disallowed here).
        let cached = host
            .read_bytes(&cache_dir.join(cache_file_name(&pinned)), MAX_DICT)
            .unwrap();
        assert_eq!(
            cached, FIXTURE,
            "the cache must store the compressed, pin-verifiable blob"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_cache_hit_self_heals_by_reprovisioning() {
        let dir = temp_dir();
        let host = NativeHost;
        let source = dir.join("ipadic.zst");
        host.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);
        let cache_path = cache_dir.join(cache_file_name(&pinned));

        provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        // Corrupt the entry (bit-rot / truncation analog): bytes that do NOT hash to the pin.
        host.write_atomic(&cache_path, b"corrupted not-a-dictionary")
            .unwrap();

        // The next hit re-verifies `blake3 == pin`, finds the mismatch, and self-heals from source.
        let healed = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        assert_eq!(healed, PAYLOAD.as_bytes());
        // …and the cache is repaired back to the verifiable compressed blob.
        assert_eq!(host.read_bytes(&cache_path, MAX_DICT).unwrap(), FIXTURE);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_cache_hit_with_no_recoverable_source_surfaces_the_error() {
        let dir = temp_dir();
        let host = NativeHost;
        let source = dir.join("ipadic.zst");
        host.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);
        let cache_path = cache_dir.join(cache_file_name(&pinned));

        provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        host.write_atomic(&cache_path, b"corrupted").unwrap();
        std::fs::remove_file(&source).unwrap();

        // Self-heal re-acquires; with the source gone the acquisition error is surfaced — the
        // corrupt bytes are NEVER served.
        let err = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap_err();
        assert!(matches!(err, DictError::Io(IoError::NotFound)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_vanished_cache_between_check_and_read_is_a_miss_not_a_fatal_error() {
        // TOCTOU: the entry passes an `exists()` hint but is gone by read time (`NotFound`).
        // Dropping the pre-check and mapping `NotFound` to a miss keeps provisioning succeeding.
        let dir = temp_dir();
        let host = CacheFaultHost {
            fs: NativeHost,
            fault: CacheFault::Vanished,
        };
        let source = dir.join("ipadic.zst");
        NativeHost.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);

        let out = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        assert_eq!(out, PAYLOAD.as_bytes());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_cache_hit_that_is_still_valid_zstd_but_wrong_hash_self_heals() {
        // Isolates the `blake3 == pin` re-check from the zstd-decompress guard: the poison entry is
        // a DIFFERENT but perfectly decodable zstd blob, so the decompressor would happily serve it
        // — only the pin re-check can reject it. (Models bit-rot landing on a still-valid frame, or
        // a stale pre-M2m entry.) Without the re-check the wrong dictionary would be served silently.
        let dir = temp_dir();
        let host = NativeHost;
        let source = dir.join("ipadic.zst");
        host.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);
        let cache_path = cache_dir.join(cache_file_name(&pinned));

        provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        // The poison is valid zstd (it decompresses cleanly) but its hash is not the pin.
        assert_eq!(
            zstd_decompress(OTHER_FIXTURE, MAX_DICT).unwrap(),
            OTHER_PAYLOAD.as_bytes()
        );
        assert_ne!(pin_of(OTHER_FIXTURE), pinned);
        host.write_atomic(&cache_path, OTHER_FIXTURE).unwrap();

        // The pin re-check (not the decompressor) catches the mismatch and self-heals from source.
        let healed = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap();
        assert_eq!(healed, PAYLOAD.as_bytes());
        assert_eq!(host.read_bytes(&cache_path, MAX_DICT).unwrap(), FIXTURE);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_genuinely_faulty_cache_read_is_surfaced_not_silently_missed() {
        // Guard: only `NotFound` (and a content hash-mismatch) become a soft miss. A real I/O fault
        // (`Other`/`TooLarge`) must surface, so a flaky disk is never masked as a re-provision.
        let dir = temp_dir();
        let host = CacheFaultHost {
            fs: NativeHost,
            fault: CacheFault::Unreadable,
        };
        let source = dir.join("ipadic.zst");
        NativeHost.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);

        let err = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap_err();
        assert!(matches!(err, DictError::Io(IoError::Other(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_oversize_cache_entry_is_surfaced_not_silently_re_provisioned() {
        // `TooLarge` on the cache read is a genuine fault, not a benign miss: it must surface, and
        // provisioning must NOT fall through to re-read the source (the returned error — rather than
        // an `Ok` from a healed re-provision — is itself proof there was no fall-through).
        let dir = temp_dir();
        let host = CacheFaultHost {
            fs: NativeHost,
            fault: CacheFault::Oversize,
        };
        let source = dir.join("ipadic.zst");
        NativeHost.write_atomic(&source, FIXTURE).unwrap();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);

        let err = provision_dictionary(&host, &cache_dir, &source, &pinned).unwrap_err();
        assert!(matches!(err, DictError::Io(IoError::TooLarge { .. })));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_url_corrupt_cache_hit_self_heals_by_refetching() {
        let dir = temp_dir();
        let cache_dir = dir.join("cache");
        let pinned = pin_of(FIXTURE);
        let cache_path = cache_dir.join(cache_file_name(&pinned));
        let host = FetchHost::serving(FIXTURE);

        provision_dictionary_from_url(&host, &cache_dir, SOURCE_URL, &pinned).unwrap();
        assert_eq!(host.fetches.get(), 1);
        // Corrupt the cache; the next hit must re-verify, miss, and re-fetch to self-heal.
        host.fs.write_atomic(&cache_path, b"corrupted").unwrap();
        let healed = provision_dictionary_from_url(&host, &cache_dir, SOURCE_URL, &pinned).unwrap();
        assert_eq!(healed, PAYLOAD.as_bytes());
        assert_eq!(
            host.fetches.get(),
            2,
            "a corrupt cache must trigger a healing re-fetch"
        );
        assert_eq!(host.read_bytes(&cache_path, MAX_DICT).unwrap(), FIXTURE);
        std::fs::remove_dir_all(&dir).ok();
    }
}
