use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub fn read_to_string_with_limit<P: AsRef<Path>>(path: P, limit: u64) -> io::Result<String> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    let mut content = String::new();
    file.take(limit + 1).read_to_string(&mut content)?;

    if content.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("File exceeds size limit of {} bytes", limit),
        ));
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "hello").unwrap();

        let content = read_to_string_with_limit(file.path(), 10).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeded() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "hello world").unwrap();

        let result = read_to_string_with_limit(file.path(), 5);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(unix)]
    fn test_read_to_string_with_limit_dev_zero() {
        // Test pseudo-files which have size 0 but produce infinite data.
        let result = read_to_string_with_limit("/dev/zero", 5);
        assert!(result.is_err());
    }
}
