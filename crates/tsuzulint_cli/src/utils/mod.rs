//! CLI utility functions

use std::fs::File;
use std::io::Read;

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn read_to_string_with_limit<P: AsRef<std::path::Path>>(
    path: P,
    limit: u64,
) -> std::io::Result<String> {
    let file = File::open(path.as_ref())?;
    let metadata = file.metadata()?;
    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
        ));
    }
    let mut reader = std::io::Read::take(file, limit + 1);
    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
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
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello").unwrap();
        let path = temp_file.path().to_path_buf();

        let content = read_to_string_with_limit(&path, 10).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_read_to_string_with_limit_metadata_too_large() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello").unwrap();
        let path = temp_file.path().to_path_buf();

        let err = read_to_string_with_limit(&path, 3).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "file too large");
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_dev_zero() {
        let path = "/dev/zero";
        // /dev/zero has size 0 but is infinite.
        let err = read_to_string_with_limit(path, 10).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "file too large");
    }
}
