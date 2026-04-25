//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use std::io::Read;
use std::path::Path;
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File exceeds size limit",
        ));
    }
    let mut buffer = String::new();
    file.take(limit + 1).read_to_string(&mut buffer)?;
    if buffer.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File exceeds size limit",
        ));
    }
    Ok(buffer)
}

pub fn read_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File exceeds size limit",
        ));
    }
    let mut buffer = Vec::new();
    file.take(limit + 1).read_to_end(&mut buffer)?;
    if buffer.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File exceeds size limit",
        ));
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let content = read_to_string_with_limit(file.path(), 100).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_limit() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let err = read_to_string_with_limit(file.path(), 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_read_with_limit_success() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let content = read_with_limit(file.path(), 100).unwrap();
        assert_eq!(content, b"hello world");
    }

    #[test]
    fn test_read_with_limit_exceeds_limit() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let err = read_with_limit(file.path(), 5).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_read_to_string_with_limit_buffer_len_exceeds_limit() {
        if std::path::Path::new("/dev/zero").exists() {
            let err = read_to_string_with_limit("/dev/zero", 10).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn test_read_with_limit_buffer_len_exceeds_limit() {
        if std::path::Path::new("/dev/zero").exists() {
            let err = read_with_limit("/dev/zero", 10).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        }
    }
}
