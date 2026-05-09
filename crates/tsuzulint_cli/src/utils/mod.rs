//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

/// Reads a file to a string with a size limit to prevent memory exhaustion
/// vulnerabilities when reading untrusted input like configs or manifests.
pub fn read_to_string_with_limit<P: AsRef<std::path::Path>>(
    path: P,
    limit: u64,
) -> std::io::Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;

    // Quick check based on metadata (can be bypassed by pseudo-files)
    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File size exceeds memory exhaustion limit",
        ));
    }

    // Read with a strict limit to protect against pseudo-files (like /dev/zero)
    // that report 0 size but produce infinite output.
    let mut content = String::with_capacity(metadata.len() as usize);
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File content exceeds memory exhaustion limit",
        ));
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        let path = tmp.path().to_path_buf();

        let result = read_to_string_with_limit(&path, 100);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello world");
    }

    #[test]
    fn test_read_to_string_with_limit_metadata_exceeded() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        let path = tmp.path().to_path_buf();

        let result = read_to_string_with_limit(&path, 5);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("File size exceeds")
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_pseudo_file() {
        // /dev/zero reports 0 size but produces infinite bytes, testing the inner read bounds
        let path = std::path::Path::new("/dev/zero");
        let result = read_to_string_with_limit(path, 10);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("File content exceeds")
        );
    }
}
