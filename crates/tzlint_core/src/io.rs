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

/// One immediate child returned by [`Host::list_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The final path component only (e.g. `"a.md"`), never a full path.
    pub name: String,
    /// What the entry resolves to. A directory walker uses this to decide whether to recurse
    /// and whether to skip symlinks.
    pub kind: EntryKind,
}

/// The classification of a [`DirEntry`]. A symlink is reported as [`EntryKind::Symlink`]
/// regardless of its target, so a walker can refuse to follow it (loop / DoS guard) without a
/// second `stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symlink (to anything). Walkers never follow these.
    Symlink,
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
    /// Whether `path` exists. Best-effort: returns `false` when existence cannot be
    /// determined (e.g. a permission error). Config discovery uses this to probe candidate
    /// files cheaply without reading them — so checking presence still goes through the
    /// boundary rather than touching `std::fs` directly.
    fn exists(&self, path: &Path) -> bool;
    /// List the immediate children of `dir` (a single level, **not** recursive). The CLI's
    /// glob / directory expansion drives recursion itself on top of this, so depth, dedup,
    /// and symlink policy stay out of the boundary.
    ///
    /// The default returns an `Err`, so a non-filesystem host (a browser/wasm embedder, the
    /// LSP scaffold) need not implement it — directory expansion simply isn't available there.
    /// [`NativeHost`] overrides it. A missing `dir` should map to [`IoError::NotFound`].
    fn list_dir(&self, dir: &Path) -> Result<Vec<DirEntry>, IoError> {
        let _ = dir;
        Err(IoError::Other(
            "directory listing is not supported by this host".to_string(),
        ))
    }
}

// The real-filesystem host. Not compiled for `wasm32`, where the embedder injects its own
// `Host` (there is no ambient filesystem in the browser).
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{DirEntry, EntryKind, Host, IoError, Path};
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

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
            // `saturating_add` so `limit == usize::MAX` ("no practical cap") reads everything
            // rather than overflowing (which would panic in debug / wrap to `take(0)` in release).
            file.take((limit as u64).saturating_add(1))
                .read_to_end(&mut bytes)?;
            if bytes.len() > limit {
                return Err(IoError::TooLarge { limit });
            }
            String::from_utf8(bytes).map_err(|e| IoError::Other(format!("invalid UTF-8: {e}")))
        }

        fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
            // If `path` is a symlink, the final `rename` replaces the link with a regular
            // file (standard atomic-write semantics); it does not write through to the link
            // target.
            let mut tmp = TempFile::create(path)?;
            tmp.write_all_synced(contents)?; // on error the guard removes the temp on drop
            tmp.commit(path)?; // rename over the target (atomic on POSIX)
            fsync_parent(path);
            Ok(())
        }

        fn exists(&self, path: &Path) -> bool {
            // `try_exists` follows symlinks and distinguishes "absent" from "couldn't tell";
            // a permission/other error is treated as "not present" (best-effort discovery).
            path.try_exists().unwrap_or(false)
        }

        fn list_dir(&self, dir: &Path) -> Result<Vec<DirEntry>, IoError> {
            // The single allowed `read_dir` call site (the io boundary); clippy's
            // `disallowed_methods` bans it everywhere else, mirroring `read`/`write`.
            #[allow(clippy::disallowed_methods)]
            let read = std::fs::read_dir(dir)?;
            let mut entries = Vec::new();
            for entry in read {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                // `file_type()` does NOT follow the link, so a symlink (even to a directory)
                // is reported as `Symlink` and the walker will not descend into it.
                let file_type = entry.file_type()?;
                let kind = if file_type.is_symlink() {
                    EntryKind::Symlink
                } else if file_type.is_dir() {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                };
                entries.push(DirEntry { name, kind });
            }
            Ok(entries)
        }
    }

    /// fsync the containing directory so a completed `rename` survives a crash (Unix only).
    fn fsync_parent(path: &Path) {
        #[cfg(unix)]
        {
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
            if let Ok(handle) = File::open(dir.unwrap_or_else(|| Path::new("."))) {
                let _ = handle.sync_all();
            }
        }
        #[cfg(not(unix))]
        let _ = path;
    }

    /// An exclusively-created temp file that is removed on drop unless [`commit`](TempFile::commit)
    /// succeeds. Created with `O_CREAT | O_EXCL` (mode `0o600` on Unix), so a pre-positioned
    /// symlink or file at the temp path is **never** followed/clobbered — closing the
    /// predictable-temp-name symlink-TOCTOU hole.
    struct TempFile {
        path: PathBuf,
        file: Option<File>,
        armed: bool,
    }

    impl TempFile {
        /// Number of fresh temp names to try before giving up.
        const ATTEMPTS: usize = 16;

        fn create(target: &Path) -> Result<Self, IoError> {
            for _ in 0..Self::ATTEMPTS {
                let path = tmp_sibling(target);
                let mut options = OpenOptions::new();
                options.write(true).create_new(true); // O_CREAT | O_EXCL: never follow an existing entry
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&path) {
                    Ok(file) => {
                        return Ok(TempFile {
                            path,
                            file: Some(file),
                            armed: true,
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            Err(IoError::Other(
                "could not create a unique temporary file".to_string(),
            ))
        }

        fn write_all_synced(&mut self, contents: &[u8]) -> Result<(), IoError> {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| IoError::Other("temporary file already committed".to_string()))?;
            file.write_all(contents)?;
            file.sync_all()?; // durably flush the data before the rename
            Ok(())
        }

        /// Close the temp file and rename it over `target`. On success the guard disarms (no
        /// cleanup); on failure the guard removes the temp on drop.
        fn commit(mut self, target: &Path) -> Result<(), IoError> {
            drop(self.file.take()); // close the handle before renaming
            std::fs::rename(&self.path, target)?;
            self.armed = false;
            Ok(())
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            if self.armed {
                drop(self.file.take());
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    /// A unique sibling temp path `<name>.tzlint-tmp.<pid>.<counter>.<nanos>` (same directory,
    /// so the rename stays on one filesystem). pid + a process-wide counter + the clock make
    /// the name hard to pre-guess; `O_EXCL` (in [`TempFile::create`]) is the actual guard.
    fn tmp_sibling(path: &Path) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let mut name = path
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_default();
        name.push(format!(".tzlint-tmp.{pid}.{n}.{nanos}"));
        path.with_file_name(name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::io::{DirEntry, EntryKind, IoError, MAX_FILE};
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
        fn write_atomic_errors_and_leaves_no_temp_when_rename_fails() {
            // Renaming the temp over an existing directory fails; the error surfaces and the
            // RAII guard removes the temp (no leak), rather than panicking.
            let dir = temp_dir();
            let target = dir.join("a-directory");
            std::fs::create_dir_all(&target).unwrap();
            assert!(NativeHost.write_atomic(&target, b"data").is_err());
            // Dogfood `list_dir` (rather than raw `read_dir`, now disallowed) to scan for a leak.
            let leaked = NativeHost
                .list_dir(&dir)
                .unwrap()
                .iter()
                .any(|e| e.name.contains("tzlint-tmp"));
            assert!(!leaked, "temp file leaked on the error path");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn write_atomic_errors_when_parent_is_missing() {
            // The temp file cannot be created (parent directory absent) → error, not panic.
            let missing = Path::new("/no/such/tzlint/dir/out.md");
            assert!(NativeHost.write_atomic(missing, b"data").is_err());
        }

        #[test]
        fn read_rejects_invalid_utf8() {
            // Raw non-UTF-8 bytes surface as `Other("invalid UTF-8: ...")`, not a panic.
            let dir = temp_dir();
            let path = dir.join("bin");
            let host = NativeHost;
            host.write_atomic(&path, &[0xff, 0xfe, 0x00]).unwrap();
            let err = host.read_to_string(&path, MAX_FILE).unwrap_err();
            assert!(
                matches!(&err, IoError::Other(m) if m.contains("invalid UTF-8")),
                "got {err:?}"
            );
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn oversize_is_reported_before_utf8_validation() {
            // Bytes that are BOTH invalid UTF-8 and over the limit must report `TooLarge`
            // (the size check runs before UTF-8 validation).
            let dir = temp_dir();
            let path = dir.join("bigbin");
            let host = NativeHost;
            host.write_atomic(&path, &[0xff; 10]).unwrap();
            assert!(matches!(
                host.read_to_string(&path, 4),
                Err(IoError::TooLarge { limit: 4 })
            ));
            std::fs::remove_dir_all(&dir).ok();
        }

        #[cfg(unix)]
        #[test]
        fn write_atomic_replaces_symlink_without_following_it() {
            // Writing through a symlink replaces the LINK with a regular file; the link
            // target is left untouched (the symlink-TOCTOU hardening claim).
            use std::os::unix::fs::symlink;
            let dir = temp_dir();
            let target = dir.join("real-target");
            let link = dir.join("link");
            let host = NativeHost;
            host.write_atomic(&target, b"original").unwrap();
            symlink(&target, &link).unwrap();
            host.write_atomic(&link, b"new").unwrap();
            assert!(
                !std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the symlink should have been replaced by a regular file"
            );
            assert_eq!(host.read_to_string(&link, MAX_FILE).unwrap(), "new");
            assert_eq!(host.read_to_string(&target, MAX_FILE).unwrap(), "original");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn exists_reports_presence() {
            let dir = temp_dir();
            let path = dir.join("present");
            let host = NativeHost;
            assert!(!host.exists(&path), "absent before creation");
            host.write_atomic(&path, b"x").unwrap();
            assert!(host.exists(&path), "present after creation");
            assert!(!host.exists(&dir.join("never")), "unrelated path absent");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn read_with_no_practical_cap() {
            // `usize::MAX` means "no practical cap": reads fully without overflow (no debug
            // panic, no release wrap-to-empty).
            let dir = temp_dir();
            let path = dir.join("f");
            let host = NativeHost;
            host.write_atomic(&path, b"content").unwrap();
            assert_eq!(host.read_to_string(&path, usize::MAX).unwrap(), "content");
            std::fs::remove_dir_all(&dir).ok();
        }

        /// Index a listing by name so assertions don't depend on `read_dir`'s arbitrary order.
        fn by_name(entries: Vec<DirEntry>) -> std::collections::BTreeMap<String, EntryKind> {
            entries.into_iter().map(|e| (e.name, e.kind)).collect()
        }

        #[test]
        fn list_dir_classifies_files_dirs_and_symlinks() {
            // A regular file is `File`, a subdirectory is `Dir`, and (on Unix) a symlink is
            // `Symlink` — classified WITHOUT following the link, so the walker can refuse it.
            let dir = temp_dir();
            let host = NativeHost;
            host.write_atomic(&dir.join("a.md"), b"x").unwrap();
            std::fs::create_dir(dir.join("sub")).unwrap();

            let entries = by_name(host.list_dir(&dir).unwrap());
            assert_eq!(entries.get("a.md"), Some(&EntryKind::File));
            assert_eq!(entries.get("sub"), Some(&EntryKind::Dir));

            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                symlink(dir.join("a.md"), dir.join("link")).unwrap();
                let entries = by_name(host.list_dir(&dir).unwrap());
                assert_eq!(
                    entries.get("link"),
                    Some(&EntryKind::Symlink),
                    "a symlink must be classified as Symlink, not its target kind"
                );
            }

            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn list_dir_missing_is_not_found() {
            // Listing a path that does not exist maps to `NotFound` (via `From<io::Error>`).
            let host = NativeHost;
            assert!(matches!(
                host.list_dir(Path::new("/no/such/tzlint/dir/zzz")),
                Err(IoError::NotFound)
            ));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeHost;
