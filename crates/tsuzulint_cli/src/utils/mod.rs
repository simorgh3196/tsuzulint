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

    #[test]
    fn test_read_to_string_with_limit_exceeds_limit_runtime() {
        let file = NamedTempFile::new().unwrap();
        // File size looks okay at first, but grows or is fake.
        // We write 100 bytes, but we will pass a `None` limit to ensure it uses default
        // Then we'll create a fake file that uses /dev/zero but since it's hard to mock metadata
        // we'll just test the `content.len() as u64 > limit` branch if possible.
        // Actually, let's just make it hit the second limit error by writing a large file
        // but modifying the metadata length or finding a way to trigger the second error.
        // Since we can't easily mock metadata in std, we'll write a file that gets larger during read,
        // or just test the first limit thoroughly and accept the code coverage, or we can use a Unix pipe/fifo
        // to bypass the metadata check.
        // Let's create a FIFO to bypass metadata.len() == 0, and write to it.
        #[cfg(unix)]
        {
            let fifo_path = file.path().with_extension("fifo");
            let _ = std::fs::remove_file(&fifo_path);
            let c_path = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
            unsafe {
                libc::mkfifo(c_path.as_ptr(), 0o644);
            }

            // Spawn a thread to write to the FIFO
            let path_clone = fifo_path.clone();
            std::thread::spawn(move || {
                if let Ok(mut f) = std::fs::File::create(&path_clone) {
                    let _ = f.write_all(&[b'a'; 100]);
                }
            });

            // Metadata len of a FIFO is usually 0.
            let result = read_to_string_with_limit(&fifo_path, Some(50));
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);

            let _ = std::fs::remove_file(&fifo_path);
        }
    }
}
