//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use std::io::Read;
use std::path::Path;
use tokio::runtime::Runtime;

/// Reads up to `limit + 1` bytes from the file at `path`.
/// Returns an error if the file exceeds `limit` bytes or cannot be read.
pub fn read_to_string_with_limit(path: &Path, limit: u64) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    let capacity_hint = file.metadata().map(|m| m.len()).unwrap_or(0).min(limit) as usize;

    let mut content = String::with_capacity(capacity_hint);
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "File size exceeds limit of {} bytes: {}",
                limit,
                path.display()
            ),
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
