//! Centralized boundary I/O. All raw file/network reads and writes must pass
//! through this module to ensure safe limits and atomicity.

use std::io::{self, Read};
use std::path::Path;

/// Reads a file into a string, up to a specified maximum size.
///
/// Prevents memory exhaustion attacks by unbounded file reads, and avoids TOCTOU
/// vulnerabilities by not checking metadata length before reading.
#[allow(clippy::disallowed_methods)]
pub fn read_with_limit(path: impl AsRef<Path>, limit: u64) -> io::Result<String> {
    let file = std::fs::File::open(path)?;
    let mut buffer = String::new();

    let read_limit = limit
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "read limit overflow"))?;

    // Read up to limit + 1 bytes. If we read limit + 1, the file is too large.
    file.take(read_limit).read_to_string(&mut buffer)?;

    if buffer.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file size exceeds limit",
        ));
    }

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_with_limit_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();

        let content = read_with_limit(temp_file.path(), 100).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_with_limit_file_too_large() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();

        let result = read_with_limit(temp_file.path(), 5);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn test_read_with_limit_exact_limit() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "12345").unwrap();

        let content = read_with_limit(temp_file.path(), 5).unwrap();
        assert_eq!(content, "12345");
    }

    #[test]
    fn test_read_with_limit_overflow() {
        let temp_file = NamedTempFile::new().unwrap();

        let result = read_with_limit(temp_file.path(), u64::MAX);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
