//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use std::io::Read;
use std::path::Path;
use tokio::runtime::Runtime;

/// Maximum size for manifest/config files (default: 1MB)
pub const DEFAULT_MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Safely read a file to a string, preventing memory exhaustion from excessively large files
pub fn read_to_string_with_limit(
    path: impl AsRef<Path>,
    limit: Option<u64>,
) -> std::io::Result<String> {
    let path = path.as_ref();
    let limit = limit.unwrap_or(DEFAULT_MAX_FILE_SIZE);

    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;

    // Check size upfront if possible
    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds maximum size limit of {} bytes", limit),
        ));
    }

    // Read with a strict bound to handle files that might grow during read
    // or pseudo-files where metadata len is inaccurate
    let mut content = String::new();
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File contents exceed maximum size limit of {} bytes", limit),
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
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "Hello, world!").unwrap();

        let result = read_to_string_with_limit(file.path(), Some(100));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, world!\n");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_limit() {
        let mut file = NamedTempFile::new().unwrap();
        let large_string = "a".repeat(100);
        writeln!(file, "{}", large_string).unwrap();

        let result = read_to_string_with_limit(file.path(), Some(50));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }
}
