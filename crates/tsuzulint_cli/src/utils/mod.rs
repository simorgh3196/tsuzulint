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

/// Reads a file to a string, returning an error if its size exceeds the limit.
pub fn read_to_string_with_limit(path: &Path, limit: u64) -> std::io::Result<String> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "File {} exceeds size limit of {} bytes",
                path.display(),
                limit
            ),
        ));
    }

    let capacity = (metadata.len() as usize).min(limit as usize);
    let mut content = String::with_capacity(capacity);
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "File {} exceeds size limit of {} bytes",
                path.display(),
                limit
            ),
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
        file.write_all(b"hello world").unwrap();
        let content = read_to_string_with_limit(file.path(), 100).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_metadata_limit() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let err = read_to_string_with_limit(file.path(), 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeds size limit"));
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_read_limit() {
        // We can simulate an issue where metadata lies or we just want to test the pseudo-file path
        // Actually, since we check metadata first, testing the second check is tricky with regular files.
        // But we can test it using /dev/zero on unix.
        #[cfg(unix)]
        {
            let err = read_to_string_with_limit(Path::new("/dev/zero"), 5).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("exceeds size limit"));
        }
    }
}
