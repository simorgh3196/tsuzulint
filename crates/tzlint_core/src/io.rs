//! Centralized boundary I/O. All raw file/network reads and writes must pass
//! through this module to ensure safe limits and atomicity.

use std::io::{self, Read};
use std::path::Path;

/// Reads a file into a string, up to a specified maximum size.
///
/// Prevents memory exhaustion attacks by unbounded file reads, and avoids TOCTOU
/// vulnerabilities by not checking metadata length before reading.
#[allow(clippy::disallowed_methods)]
pub fn read_with_limit(path: impl AsRef<Path>, limit: u64) -> io::Result<String> {
    let file = std::fs::File::open(path)?;
    let mut buffer = String::new();

    // Read up to limit + 1 bytes. If we read limit + 1, the file is too large.
    file.take(limit + 1).read_to_string(&mut buffer)?;

    if buffer.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file size exceeds limit",
        ));
    }

    Ok(buffer)
}
