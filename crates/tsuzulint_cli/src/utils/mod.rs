//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

use std::path::Path;

/// Reads a file to a string with a size limit (default 1MB if none provided).
/// Prevents memory exhaustion when parsing malicious or enormous config files/manifests.
pub fn read_to_string_with_limit<P: AsRef<Path>>(
    path: P,
    limit_bytes: Option<usize>,
) -> std::io::Result<String> {
    use std::fs::File;
    use std::io::Read;

    let limit = limit_bytes.unwrap_or(1024 * 1024); // 1MB default
    let file = File::open(path)?;
    let mut content = String::new();

    // Check metadata len first as a fast path
    if let Ok(metadata) = file.metadata()
        && metadata.len() > limit as u64
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File too large. Exceeds {} byte limit", limit),
        ));
    }

    file.take((limit + 1) as u64).read_to_string(&mut content)?;

    if content.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File too large. Exceeds {} byte limit", limit),
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
        let res = read_to_string_with_limit(tmp.path(), Some(100)).unwrap();
        assert_eq!(res, "hello world");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_metadata() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        let err = read_to_string_with_limit(tmp.path(), Some(5)).unwrap_err();
        assert!(err.to_string().contains("File too large"));
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_pseudo_file() {
        // pseudo-file /dev/zero has size 0 in metadata but infinite content
        let err = read_to_string_with_limit("/dev/zero", Some(10)).unwrap_err();
        assert!(err.to_string().contains("File too large"));
    }
}
