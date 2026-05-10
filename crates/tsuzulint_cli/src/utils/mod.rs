//! CLI utility functions

use std::fs::File;
use std::io::Read;

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn read_to_string_with_limit<P: AsRef<std::path::Path>>(
    path: P,
    limit: u64,
) -> std::io::Result<String> {
    let file = File::open(path.as_ref())?;
    let metadata = file.metadata()?;
    if metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
        ));
    }
    let mut reader = std::io::Read::take(file, limit + 1);
    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    if content.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
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
