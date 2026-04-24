#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_to_string_with_limit_success() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "Hello, world!").unwrap();

        let result = crate::utils::read_to_string_with_limit(file.path(), Some(100));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, world!\n");
    }

    #[test]
    fn test_read_to_string_with_limit_exceeds_limit() {
        let mut file = NamedTempFile::new().unwrap();
        let large_string = "a".repeat(100);
        writeln!(file, "{}", large_string).unwrap();

        let result = crate::utils::read_to_string_with_limit(file.path(), Some(50));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
