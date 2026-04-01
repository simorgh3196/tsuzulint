import re

with open("crates/tsuzulint_core/src/file_linter.rs", "r") as f:
    content = f.read()

test_code = """
    #[test]
    #[cfg(unix)]
    fn test_clear_nonblocking() {
        use std::os::unix::io::AsFd;
        let file = tempfile::NamedTempFile::new().unwrap().into_file();
        let fd = file.as_fd();
        rustix::fs::fcntl_setfl(fd, rustix::fs::OFlags::NONBLOCK).unwrap();
        super::clear_nonblocking(&file).unwrap();
        let flags = rustix::fs::fcntl_getfl(fd).unwrap();
        assert!(!flags.contains(rustix::fs::OFlags::NONBLOCK));
    }
"""

content = content.replace("mod tests {", "mod tests {\n" + test_code)

with open("crates/tsuzulint_core/src/file_linter.rs", "w") as f:
    f.write(content)
