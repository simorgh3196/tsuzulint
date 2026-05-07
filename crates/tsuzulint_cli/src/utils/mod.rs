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

/// Reads a file to a string with a size limit to prevent memory exhaustion.
pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> std::io::Result<String> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    // Check file size from metadata if available (prevents allocation attempt for huge regular files)
    if metadata.is_file() && metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    let mut content = String::new();
    // Read up to limit + 1 bytes. If we read limit + 1, it's too big (e.g. pseudo files where len=0)
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_to_string_with_limit() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        let content = "Hello, world!";
        std::fs::write(&file_path, content).unwrap();

        // Under limit
        let result = read_to_string_with_limit(&file_path, 100).unwrap();
        assert_eq!(result, content);

        // Exact limit
        let result = read_to_string_with_limit(&file_path, content.len() as u64).unwrap();
        assert_eq!(result, content);

        // Over limit
        let err = read_to_string_with_limit(&file_path, (content.len() - 1) as u64).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[test]
    fn test_read_to_string_with_limit_pseudo_file() {
        // /dev/zero reports 0 size but provides infinite bytes
        let err = read_to_string_with_limit("/dev/zero", 100).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
