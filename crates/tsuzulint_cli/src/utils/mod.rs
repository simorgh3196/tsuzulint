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

/// Reads a file to a string with a maximum size limit to prevent memory exhaustion
pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> std::io::Result<String> {
    let file = File::open(path.as_ref())?;

    // Check metadata first if possible
    if let Ok(metadata) = file.metadata() {
        if metadata.len() > limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("File too large (exceeds {} bytes)", limit),
            ));
        }
    }

    // Read with explicit limit to protect against pseudo-files (/dev/zero)
    let mut content = String::new();
    let read_count = file.take(limit + 1).read_to_string(&mut content)?;

    if read_count as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File too large (exceeds {} bytes)", limit),
        ));
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "Hello, World!").unwrap();
        let content = read_to_string_with_limit(file.path(), 100).unwrap();
        assert_eq!(content, "Hello, World!");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_metadata() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "Hello, World!").unwrap();
        let result = read_to_string_with_limit(file.path(), 5);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds 5 bytes"));
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_pseudo_file() {
        // /dev/zero has metadata length 0, but reads forever
        let result = read_to_string_with_limit("/dev/zero", 10);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds 10 bytes"));
    }
}
