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
