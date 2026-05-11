//! CLI utility functions

use miette::{IntoDiagnostic, Result};
use tokio::runtime::Runtime;

pub fn create_tokio_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()
}

pub fn read_to_string_with_limit<P: AsRef<std::path::Path>>(
    path: P,
    max_size: u64,
) -> std::io::Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buffer = String::new();
    file.take(max_size + 1).read_to_string(&mut buffer)?;
    if buffer.len() as u64 > max_size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File too large",
        ));
    }
    Ok(buffer)
}
