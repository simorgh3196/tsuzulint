//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

/// Reads a file into a string with a size limit to prevent memory exhaustion
pub fn read_to_string_with_limit<P: AsRef<std::path::Path>>(
    path: P,
    limit: u64,
) -> std::io::Result<String> {
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File size exceeds limit of {} bytes", limit),
        ));
    }

    let mut content = String::new();
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File content exceeds limit of {} bytes", limit),
        ));
    }

    Ok(content)
}

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();

        let content = read_to_string_with_limit(tmp.path(), 100).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_to_string_with_limit_metadata_exceeded() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();

        // Limit is 5, but file is 11 bytes
        let err = read_to_string_with_limit(tmp.path(), 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("size exceeds limit"));
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_content_exceeded() {
        // We need a file where metadata.len() is 0 but it has content.
        // /dev/zero is perfect for this, but we only want to read up to limit + 1
        let err = read_to_string_with_limit("/dev/zero", 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("content exceeds limit"));
    }
}
