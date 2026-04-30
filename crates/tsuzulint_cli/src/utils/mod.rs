//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

/// Reads a file to a string, enforcing a maximum size limit to prevent memory exhaustion.
pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;

    // Check initial metadata size
    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    // Check actual read size to prevent pseudo-file bypasses (like /dev/zero)
    let mut buffer = String::new();
    file.by_ref().take(limit + 1).read_to_string(&mut buffer)?;

    if buffer.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File stream exceeds size limit of {} bytes", limit),
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
    fn test_read_success() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "hello world").unwrap();
        let path = file.path();

        let result = read_to_string_with_limit(path, 100).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_read_metadata_limit_exceeded() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "hello world").unwrap(); // 11 bytes
        let path = file.path();

        let err = read_to_string_with_limit(path, 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds size limit"));
    }

    #[test]
    #[cfg(unix)]
    fn test_read_stream_limit_exceeded() {
        // /dev/zero has 0 metadata length but infinite stream
        let path = Path::new("/dev/zero");
        let err = read_to_string_with_limit(path, 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("File stream exceeds size limit"));
    }
}
