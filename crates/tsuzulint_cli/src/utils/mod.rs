//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

/// Reads the contents of a file to a string with a maximum size limit.
/// Protects against memory exhaustion vulnerabilities from arbitrarily large files.
/// Includes safeguards against pseudo-files (like /dev/zero) and concurrently growing files.
pub fn read_to_string_with_limit(path: &std::path::Path, limit: u64) -> std::io::Result<String> {
    use std::fs::File;
    use std::io::{Error, ErrorKind, Read};

    let file = File::open(path)?;
    let metadata = file.metadata()?;

    // Fast path: Check metadata if available and reliable
    if metadata.len() > limit {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "File too large. Size {} exceeds limit {}",
                metadata.len(),
                limit
            ),
        ));
    }

    // Slow path: Read with a strict bound (limit + 1 to detect overflow)
    // This protects against files where metadata.len() is 0 or inaccurate (e.g., /dev/zero or /proc files)
    let mut buffer = String::new();
    file.take(limit + 1).read_to_string(&mut buffer)?;

    if buffer.len() as u64 > limit {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("File too large. Content exceeds limit {}", limit),
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
    fn test_read_to_string_with_limit_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();

        let result = read_to_string_with_limit(temp_file.path(), 100).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_metadata() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();

        // Limit is 5, but file is 11 bytes. Should fail fast on metadata check.
        let err = read_to_string_with_limit(temp_file.path(), 5).unwrap_err();
        assert!(err.to_string().contains("File too large"));
    }

    #[cfg(unix)]
    #[test]
    fn test_read_to_string_with_limit_pseudo_file() {
        // /dev/zero has metadata.len() == 0, but provides infinite bytes.
        // The .take(limit + 1) should catch it.
        let path = std::path::Path::new("/dev/zero");

        let err = read_to_string_with_limit(path, 10).unwrap_err();
        assert!(err.to_string().contains("Content exceeds limit"));
    }
}
