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

/// Reads a file to a string with a maximum size limit to prevent memory exhaustion
pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> std::io::Result<String> {
    let file = File::open(path.as_ref())?;

    // Check metadata first if possible
    if let Ok(metadata) = file.metadata() {
        if metadata.len() > limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("File too large (exceeds {} bytes)", limit),
            ));
        }
    }

    // Read with explicit limit to protect against pseudo-files (/dev/zero)
    let mut content = String::new();
    let read_count = file.take(limit + 1).read_to_string(&mut content)?;

    if read_count as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File too large (exceeds {} bytes)", limit),
        ));
    }

    Ok(content)
}
