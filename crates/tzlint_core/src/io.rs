use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Reads a file strictly up to `limit` bytes.
/// Uses `Read::take(limit + 1)` to avoid TOCTOU (no metadata checking before reading).
/// Fails if the file is larger than the limit.
#[allow(clippy::disallowed_methods)] // Allowed: this is the centralized I/O boundary.
pub fn read_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    // Read up to limit + 1 bytes. If we read limit + 1, it means the file is too large.
    let mut buf = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut buf)?;

    if buf.len() > limit as usize {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("file exceeded limit of {} bytes", limit),
        ));
    }

    Ok(buf)
}

/// Reads a file strictly up to `limit` bytes and returns it as a String.
pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> io::Result<String> {
    let buf = read_with_limit(path, limit)?;
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes `data` to `path` atomically and durably.
///
/// 1. Opens a temporary file in the same directory (`O_CREAT | O_EXCL`).
/// 2. Sets permissions to 0600 on Unix.
/// 3. Writes data and calls `fsync`.
/// 4. Renames the temporary file over the target `path`.
/// 5. Calls `fsync` on the parent directory to ensure the directory entry is durably written.
#[allow(clippy::disallowed_methods)] // Allowed: this is the centralized I/O boundary.
pub fn atomic_write<P: AsRef<Path>>(path: P, data: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    // Generate a temporary file path in the same directory to ensure `rename` doesn't cross mount points.
    let mut tmp_path: PathBuf = parent.to_path_buf();
    let mut tmp_name = path
        .file_name()
        .map(|os_str| os_str.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("tmp"));
    tmp_name.push(".tmp.");
    // We use a timestamp for simple uniqueness
    tmp_name.push(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_nanos()
            .to_string(),
    );
    tmp_path.push(tmp_name);

    // Open temporary file with O_CREAT | O_EXCL
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600); // 0600 permissions
    }

    let mut tmp_file = options.open(&tmp_path)?;

    // Write and fsync
    if let Err(e) = tmp_file.write_all(data) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Err(e) = tmp_file.sync_all() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Rename tmp file to target path
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // Fsync the parent directory
    let dir = File::open(parent)?;
    dir.sync_all()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_with_limit() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("tzlint_io_test_read_with_limit.txt");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"Hello").unwrap();

        assert_eq!(read_with_limit(&path, 5).unwrap(), b"Hello");
        assert_eq!(read_with_limit(&path, 10).unwrap(), b"Hello");
        assert!(read_with_limit(&path, 4).is_err());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_atomic_write() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("tzlint_io_test_atomic_write.txt");

        atomic_write(&path, b"Secure atomic data").unwrap();
        let data = read_with_limit(&path, 100).unwrap();
        assert_eq!(data, b"Secure atomic data");

        std::fs::remove_file(path).unwrap();
    }
}
