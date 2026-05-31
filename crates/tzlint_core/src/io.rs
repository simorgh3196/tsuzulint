//! Centralized boundary I/O — the single place raw filesystem access is allowed.
//!
//! Everything else reaches the filesystem only through the [`Host`] abstraction (enforced
//! by the `io-guard` CI job and clippy `disallowed-methods`), so embedders can inject their
//! environment (native fs / Node / browser) and so size limits and atomic writes apply
//! uniformly. Size constants live here too, in one place.

use std::fmt;
use std::path::Path;

/// Maximum size, in bytes, of a source file read for linting. (Span offsets are `u32`, so
/// the hard ceiling is 4 GiB; this is the far smaller practical default.)
pub const MAX_FILE: usize = 16 * 1024 * 1024; // 16 MiB
/// Maximum size, in bytes, of a configuration file.
pub const MAX_CONFIG: usize = 1024 * 1024; // 1 MiB

/// A boundary-I/O failure.
#[derive(Debug)]
pub enum IoError {
    /// The path did not exist.
    NotFound,
    /// The file exceeded the byte `limit`.
    TooLarge { limit: usize },
    /// Any other I/O failure, with a human-readable reason.
    Other(String),
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::NotFound => write!(f, "file not found"),
            IoError::TooLarge { limit } => write!(f, "file exceeds the {limit}-byte limit"),
            IoError::Other(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for IoError {}

impl From<std::io::Error> for IoError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound,
            _ => IoError::Other(error.to_string()),
        }
    }
}

#[cfg(test)]
mod error_tests {
    use super::IoError;

    #[test]
    fn display_messages() {
        assert_eq!(IoError::NotFound.to_string(), "file not found");
        assert_eq!(
            IoError::TooLarge { limit: 4 }.to_string(),
            "file exceeds the 4-byte limit"
        );
        assert_eq!(IoError::Other("boom".into()).to_string(), "boom");
    }

    #[test]
    fn from_io_error_maps_kind() {
        let not_found = IoError::from(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(matches!(not_found, IoError::NotFound));
        let other = IoError::from(std::io::Error::other("x"));
        assert!(matches!(other, IoError::Other(_)));
    }
}

/// The environment a TsuzuLint host provides: bounded reads and atomic writes.
///
/// Embedders (native CLI/LSP, Node, browser) implement this so the core never touches the
/// filesystem directly. The native implementation is [`NativeHost`].
pub trait Host {
    /// Read the UTF-8 file at `path`, rejecting anything larger than `limit` bytes.
    fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError>;
    /// Atomically replace `path` with `contents` (no torn file on crash).
    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError>;
}

// The real-filesystem host. Not compiled for `wasm32`, where the embedder injects its own
// `Host` (there is no ambient filesystem in the browser).
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{Host, IoError, Path};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A [`Host`](super::Host) backed by the real filesystem (`std::fs`). The default for the
    /// native CLI and LSP.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct NativeHost;

    impl Host for NativeHost {
        fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
            let file = File::open(path)?;
            // Read raw bytes capped one past the limit, so we can distinguish "exactly at
            // the limit" from "over it" and report `TooLarge` *before* UTF-8 validation.
            let mut bytes = Vec::new();
            file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > limit {
                return Err(IoError::TooLarge { limit });
            }
            String::from_utf8(bytes).map_err(|e| IoError::Other(format!("invalid UTF-8: {e}")))
        }

        fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
            let tmp = tmp_sibling(path);
            {
                let mut file = File::create(&tmp)?;
                file.write_all(contents)?;
                file.sync_all()?; // durably flush the data before the rename
            }
            // `rename` over the destination is atomic on POSIX.
            if let Err(error) = std::fs::rename(&tmp, path) {
                let _ = std::fs::remove_file(&tmp); // best-effort cleanup
                return Err(error.into());
            }
            // fsync the directory so the rename itself survives a crash (Unix only).
            #[cfg(unix)]
            if let Some(dir) = path.parent()
                && let Ok(dir_file) = File::open(if dir.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    dir
                })
            {
                let _ = dir_file.sync_all();
            }
            Ok(())
        }
    }

    /// A unique sibling temp path `<name>.tzlint-tmp.<pid>.<counter>` (same directory, so the
    /// rename stays on one filesystem). Uniqueness uses pid + a process-wide counter — no
    /// clock or RNG needed.
    fn tmp_sibling(path: &Path) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let mut name = path
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_default();
        name.push(format!(".tzlint-tmp.{pid}.{n}"));
        path.with_file_name(name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::io::{IoError, MAX_FILE};
        use std::sync::atomic::{AtomicU64, Ordering};

        /// A unique, freshly-created temp directory (cleaned up by the caller).
        fn temp_dir() -> PathBuf {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "tzlint-io-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn write_then_read_roundtrips() {
            let dir = temp_dir();
            let path = dir.join("a.md");
            let host = NativeHost;
            host.write_atomic(&path, "見出し\n".as_bytes()).unwrap();
            assert_eq!(host.read_to_string(&path, MAX_FILE).unwrap(), "見出し\n");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn write_atomic_overwrites_existing() {
            let dir = temp_dir();
            let path = dir.join("x");
            let host = NativeHost;
            host.write_atomic(&path, b"old").unwrap();
            host.write_atomic(&path, b"new").unwrap();
            assert_eq!(host.read_to_string(&path, MAX_FILE).unwrap(), "new");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn read_rejects_oversize() {
            let dir = temp_dir();
            let path = dir.join("big");
            let host = NativeHost;
            host.write_atomic(&path, b"0123456789").unwrap();
            assert!(matches!(
                host.read_to_string(&path, 4),
                Err(IoError::TooLarge { limit: 4 })
            ));
            // Exactly at the limit is accepted.
            assert_eq!(host.read_to_string(&path, 10).unwrap(), "0123456789");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn read_missing_path_is_not_found() {
            let host = NativeHost;
            let missing = Path::new("/no/such/tzlint/path/zzz.md");
            assert!(matches!(
                host.read_to_string(missing, MAX_FILE),
                Err(IoError::NotFound)
            ));
        }

        #[test]
        fn write_atomic_errors_when_target_is_a_directory() {
            // Renaming the temp file over an existing directory fails; the error surfaces
            // (and the temp file is cleaned up) rather than panicking.
            let dir = temp_dir();
            let target = dir.join("a-directory");
            std::fs::create_dir_all(&target).unwrap();
            assert!(NativeHost.write_atomic(&target, b"data").is_err());
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeHost;
