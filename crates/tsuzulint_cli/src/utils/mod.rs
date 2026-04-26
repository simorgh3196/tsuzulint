//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

/// Reads a file into a string with a size limit to prevent memory exhaustion
pub fn read_to_string_with_limit(path: &std::path::Path, limit: u64) -> std::io::Result<String> {
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File size exceeds limit of {} bytes", limit)
        ));
    }

    let mut content = String::new();
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File content exceeds limit of {} bytes", limit)
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
