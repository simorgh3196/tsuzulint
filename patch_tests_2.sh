cat << 'INNER_EOF' >> crates/tsuzulint_cli/src/utils/mod.rs

    #[test]
    fn test_read_to_string_with_limit_buffer_len_exceeds_limit() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Since we can't easily mock metadata size < actual size locally without a pseudo file,
        // we write less than limit to bypass the first metadata check
        // then write more than limit directly to the file via another descriptor
        // Actually, we can use a small limit and bypass metadata check by creating a file
        // that's small, keeping the handle, appending more data from outside, and then reading.
        // Even simpler: just test the coverage of the second error branch

        use std::io::Write;
        // Let's just create a file that's large, but mock or somehow hit that logic.
        // Easiest way to hit the second error branch is to pass a limit smaller than the file,
        // BUT the first check catches it.
        // Wait, what if we use `/dev/zero`? /dev/zero has size 0, but infinite data.
        if std::path::Path::new("/dev/zero").exists() {
            let err = read_to_string_with_limit("/dev/zero", 10).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn test_read_with_limit_buffer_len_exceeds_limit() {
        if std::path::Path::new("/dev/zero").exists() {
            let err = read_with_limit("/dev/zero", 10).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        }
    }
INNER_EOF
