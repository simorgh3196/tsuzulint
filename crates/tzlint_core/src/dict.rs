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
//! so a tampered or wrong file is rejected without processing its contents. The decompressed
//! dictionary is cached **hash-addressed** (filename = hex of the pinned hash), so a dictionary
//! upgrade — which changes the pinned hash — is a different cache file and never serves the old
//! one (the cache-invalidation property the dictionary-version tests rely on).

use std::io::Read;
use std::path::Path;

use ruzstd::decoding::StreamingDecoder;

use crate::io::{Host, IoError, MAX_DICT};

/// A failure while provisioning a dictionary.
#[derive(Debug)]
pub enum DictError {
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
            _ => None,
        }
    }
}

impl From<IoError> for DictError {
    fn from(error: IoError) -> Self {
        DictError::Io(error)
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
/// dictionary repeatedly should hold onto the returned bytes: the cache-hit path below is an
/// `O(size)` read, so this is a provision-once-then-reuse API, not a per-use lookup. (A native
/// backend that would rather mmap the cached file can grow a path-returning variant later, when
/// such a consumer actually lands — there is none yet.)
///
/// **Cache hit:** if the decompressed result is already cached under `cache_dir`, read and return
/// it. The cache is hash-addressed and was written by a prior successful verification, so the
/// cached bytes correspond to exactly this pinned blob (the cache is trusted within its directory,
/// like the document cache). A failed hit-read — a corrupt, truncated, or over-`MAX_DICT` cache
/// entry — is **surfaced as an error**, not silently treated as a miss: the operator should clear
/// the cache, matching the cache-write stance below (the document cache, by contrast, rebuilds
/// best-effort; a soft-miss self-heal here is a deliberate policy left for when the registry lands).
///
/// **Cache miss:** read the compressed blob from `compressed_source` through the [`Host`], verify
/// `blake3(blob) == pinned_hash` **before** decompressing (never process an unverified blob),
/// zstd-decompress it (bounded by [`MAX_DICT`]), cache the result (creating `cache_dir` and
/// writing atomically), and return it. A cache-write failure is **surfaced**, not swallowed:
/// provisioning is a setup step, so a non-writable cache directory is an environment problem the
/// operator should fix rather than have masked by silently re-decompressing on every run.
pub fn provision_dictionary(
    host: &dyn Host,
    cache_dir: &Path,
    compressed_source: &Path,
    pinned_hash: &[u8; 32],
) -> Result<Vec<u8>, DictError> {
    let cache_path = cache_dir.join(cache_file_name(pinned_hash));
    if host.exists(&cache_path) {
        return Ok(host.read_bytes(&cache_path, MAX_DICT)?);
    }

    let compressed = host.read_bytes(compressed_source, MAX_DICT)?;
    // Verify before decompressing: never process the contents of an unverified blob.
    if blake3::hash(&compressed).as_bytes() != pinned_hash {
        return Err(DictError::HashMismatch);
    }
    let decompressed = zstd_decompress(&compressed, MAX_DICT)?;

    host.create_dir_all(cache_dir)?;
    host.write_atomic(&cache_path, &decompressed)?;
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
    fn dict_error_display_and_source() {
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
    }
}
