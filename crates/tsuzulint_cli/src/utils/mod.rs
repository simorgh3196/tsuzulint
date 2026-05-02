//! CLI utility functions

use std::fs;
use std::io::Read;
use std::path::Path;
use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

/// Reads up to `limit` bytes from a file.
/// Protects against memory exhaustion vulnerabilities from arbitrarily large files
/// or pseudo-files like /dev/zero.
pub fn read_to_string_with_limit(path: &Path, limit: u64) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;

    // Check initial reported metadata length if it's a regular file
    if meta.is_file() && meta.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    // Read up to limit + 1 bytes to accurately detect if it's too large,
    // which protects against /dev/zero and similar pseudo-files that report size 0
    let mut buf = String::new();
    let read_size = file.by_ref().take(limit + 1).read_to_string(&mut buf)?;

    if read_size as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    Ok(buf)
}
