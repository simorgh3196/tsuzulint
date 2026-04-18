//! Safe file I/O helpers shared between the linter and the fixer.
//!
//! This module centralises the checks that must be performed when opening and
//! reading user-provided files:
//!
//! - [`open_nonblocking`] opens a file with `O_NONBLOCK` on Unix so that a
//!   FIFO / named pipe does not block the process indefinitely.  After the
//!   caller has validated the file kind with [`check_file_metadata`], the
//!   flag must be cleared via [`clear_nonblocking`] before performing a
//!   blocking read.
//! - [`check_file_metadata`] rejects non-regular files and files whose
//!   reported size exceeds a configurable limit.
//! - [`check_limit`] is the size-bound check that is also used again after
//!   reading, because some pseudo-files (e.g. `/proc/version`, `/dev/zero`)
//!   report a size of 0 while producing arbitrarily many bytes.
//! - [`read_to_string_bounded`] reads a file into a `String` with a hard cap,
//!   returning a descriptive [`LinterError`] on failure.
//! - [`handle_io_err`] converts a `std::io::Result` into a
//!   `Result<_, LinterError>` with a consistent error message format.
//!
//! Keeping these helpers in a single module avoids subtle divergence between
//! the linter and the fixer paths (both of which must harden against the
//! same class of TOCTOU / resource-exhaustion attacks).

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::error::LinterError;

/// Opens a file with `O_NONBLOCK` on Unix so that opening a FIFO (or other
/// special file) does not block.  This also closes the TOCTOU window between
/// a pre-open metadata check and the actual open call — the caller should
/// subsequently validate the opened file with [`check_file_metadata`] (which
/// uses `fstat` semantics via `File::metadata`).
///
/// Before performing blocking reads, the caller **must** clear `O_NONBLOCK`
/// via [`clear_nonblocking`] to avoid spurious `EAGAIN` errors.
///
/// On non-Unix platforms this is a plain `fs::File::open`.
pub(crate) fn open_nonblocking(path: &Path) -> std::io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        fs::File::open(path)
    }
}

/// Clears the `O_NONBLOCK` flag on an open file descriptor so that subsequent
/// reads block normally.  This is a no-op on non-Unix platforms.
#[cfg(unix)]
pub(crate) fn clear_nonblocking(file: &fs::File) -> std::io::Result<()> {
    use std::os::unix::io::AsFd;
    let fd = file.as_fd();
    let flags = rustix::fs::fcntl_getfl(fd)?;
    let new_flags = flags - rustix::fs::OFlags::NONBLOCK;
    rustix::fs::fcntl_setfl(fd, new_flags)?;
    Ok(())
}

/// Non-Unix no-op.
#[cfg(not(unix))]
pub(crate) fn clear_nonblocking(_file: &fs::File) -> std::io::Result<()> {
    Ok(())
}

/// Converts a `std::io::Result` into a `Result<_, LinterError>` with a
/// consistent `"<msg> <path>: <err>"` message format.
pub(crate) fn handle_io_err<T>(
    res: std::io::Result<T>,
    path: &Path,
    msg: &str,
) -> Result<T, LinterError> {
    res.map_err(|e| LinterError::file(format!("{} {}: {}", msg, path.display(), e)))
}

/// Checks that `size` does not exceed `limit`, producing a descriptive
/// `LinterError` otherwise.
pub(crate) fn check_limit(size: u64, limit: u64, path: &Path) -> Result<(), LinterError> {
    if size > limit {
        Err(LinterError::file(format!(
            "File size exceeds limit of {} bytes: {}",
            limit,
            path.display()
        )))
    } else {
        Ok(())
    }
}

/// Verifies that `metadata` describes a regular file (rejects directories,
/// sockets, FIFOs on platforms that surface them as non-regular, etc.) and
/// that its reported size is within `max_size`.
///
/// Note: some pseudo-files report size 0 while yielding many bytes when read;
/// callers must therefore **also** check the content length after reading via
/// [`check_limit`].
pub(crate) fn check_file_metadata(
    metadata: &fs::Metadata,
    max_size: u64,
    path: &Path,
) -> Result<(), LinterError> {
    if !metadata.is_file() {
        return Err(LinterError::file(format!(
            "Not a regular file: {}",
            path.display()
        )));
    }
    check_limit(metadata.len(), max_size, path)
}

/// Reads up to `max_size + 1` bytes from `file` into a freshly allocated
/// `String` and rejects the result if the decoded content exceeds `max_size`
/// bytes.  The caller is responsible for clearing `O_NONBLOCK` (via
/// [`clear_nonblocking`]) before calling this function; if `O_NONBLOCK` is
/// still set on Unix, the returned error explicitly names the condition so
/// that operators can diagnose it.
///
/// The `+ 1` trick lets us distinguish "file was exactly `max_size` bytes"
/// from "file was larger than the limit" after the read completes.
pub(crate) fn read_to_string_bounded(
    file: &mut fs::File,
    max_size: u64,
    path: &Path,
) -> Result<String, LinterError> {
    let capacity_hint = file
        .metadata()
        .map(|m| m.len() as usize)
        .unwrap_or(0)
        .min(max_size as usize);
    let mut content = String::with_capacity(capacity_hint);
    file.take(max_size + 1)
        .read_to_string(&mut content)
        .map_err(|e| {
            // EAGAIN / EWOULDBLOCK can theoretically appear if O_NONBLOCK was
            // not successfully cleared; surface a clear error in that case.
            #[cfg(unix)]
            if e.raw_os_error() == Some(libc::EAGAIN) {
                return LinterError::file(format!(
                    "Failed to read {} (EAGAIN: O_NONBLOCK still set)",
                    path.display()
                ));
            }
            LinterError::file(format!("Failed to read {}: {}", path.display(), e))
        })?;

    check_limit(content.len() as u64, max_size, path)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn handle_io_err_maps_err() {
        let err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let res: Result<(), _> = handle_io_err(Err(err), Path::new("a.txt"), "Failed to open");
        let msg = res.unwrap_err().to_string();
        assert!(msg.contains("Failed to open"), "unexpected: {msg}");
        assert!(msg.contains("a.txt"), "unexpected: {msg}");
    }

    #[test]
    fn handle_io_err_passes_ok() {
        let ok: Result<u32, _> = handle_io_err(Ok(42), Path::new("a.txt"), "Failed");
        assert_eq!(ok.unwrap(), 42);
    }

    #[test]
    fn check_limit_rejects_oversize() {
        assert!(check_limit(10, 5, Path::new("a.txt")).is_err());
    }

    #[test]
    fn check_limit_accepts_equal() {
        assert!(check_limit(5, 5, Path::new("a.txt")).is_ok());
    }

    #[test]
    fn check_file_metadata_rejects_directory() {
        let dir = tempfile::tempdir().unwrap();
        let meta = std::fs::metadata(dir.path()).unwrap();
        let err = check_file_metadata(&meta, 100, dir.path()).unwrap_err();
        assert!(err.to_string().contains("Not a regular file"));
    }

    #[test]
    fn check_file_metadata_accepts_small_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let meta = std::fs::metadata(file.path()).unwrap();
        assert!(check_file_metadata(&meta, 100, file.path()).is_ok());
    }

    #[test]
    fn read_to_string_bounded_reads_content() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello").unwrap();
        let path = tmp.path().to_path_buf();
        let mut file = fs::File::open(&path).unwrap();
        let s = read_to_string_bounded(&mut file, 100, &path).unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn read_to_string_bounded_rejects_oversize() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"123456789").unwrap();
        let path = tmp.path().to_path_buf();
        let mut file = fs::File::open(&path).unwrap();
        let err = read_to_string_bounded(&mut file, 4, &path).unwrap_err();
        assert!(err.to_string().contains("exceeds limit"));
    }

    #[test]
    #[cfg(unix)]
    fn open_nonblocking_opens_regular_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hi").unwrap();
        let file = open_nonblocking(tmp.path()).unwrap();
        assert!(file.metadata().unwrap().is_file());
    }

    #[test]
    #[cfg(unix)]
    fn clear_nonblocking_clears_flag() {
        use std::os::unix::io::AsFd;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let file = open_nonblocking(tmp.path()).unwrap();
        let flags_before = rustix::fs::fcntl_getfl(file.as_fd()).unwrap();
        assert!(flags_before.contains(rustix::fs::OFlags::NONBLOCK));
        clear_nonblocking(&file).unwrap();
        let flags_after = rustix::fs::fcntl_getfl(file.as_fd()).unwrap();
        assert!(!flags_after.contains(rustix::fs::OFlags::NONBLOCK));
    }

    /// FIFO opens succeed immediately with `O_NONBLOCK` (no writer attached)
    /// and subsequent reads return EOF rather than blocking.  The linter
    /// rejects the FIFO at [`check_file_metadata`] because it is not a
    /// regular file.
    #[test]
    #[cfg(unix)]
    fn open_nonblocking_does_not_block_on_fifo() {
        use std::os::unix::fs::FileTypeExt as _;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");

        // Create the FIFO via libc; skip the test on platforms where mkfifo
        // is unavailable (e.g. exotic targets).
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) };
        if rc != 0 {
            eprintln!("mkfifo failed; skipping");
            return;
        }

        let file = open_nonblocking(&fifo).expect("O_NONBLOCK open of FIFO must not block");
        let meta = file.metadata().unwrap();
        assert!(meta.file_type().is_fifo());
        assert!(!meta.is_file());

        let err = check_file_metadata(&meta, 100, &fifo).unwrap_err();
        assert!(err.to_string().contains("Not a regular file"));
    }
}
