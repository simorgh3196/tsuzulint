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
