//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use std::fs;
use std::io::Read;
use std::path::Path;
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

/// Reads up to `limit` bytes from a file.
/// Protects against memory exhaustion vulnerabilities from arbitrarily large files
/// or pseudo-files like /dev/zero.
pub fn read_to_string_with_limit(path: &Path, limit: u64) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;

    // Check initial reported metadata length if it's a regular file
    if meta.is_file() && meta.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    // Read up to limit + 1 bytes to accurately detect if it's too large,
    // which protects against /dev/zero and similar pseudo-files that report size 0
    let mut buf = String::new();
    let read_size = file.by_ref().take(limit + 1).read_to_string(&mut buf)?;

    if read_size as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello").unwrap();
        let path = temp_file.path();

        let result = read_to_string_with_limit(path, 100).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_metadata_limit() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "123456789").unwrap();
        let path = temp_file.path();

        let result = read_to_string_with_limit(path, 4);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds size limit")
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_pseudo_file() {
        let path = Path::new("/dev/zero");
        // /dev/zero will report 0 size, so it will bypass the metadata check.
        // It should then be caught by the limit + 1 byte read check.
        let result = read_to_string_with_limit(path, 10);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds size limit")
        );
    }
}
