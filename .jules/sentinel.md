## 2025-03-31 - [Config OOM and TOCTOU fix]
**Vulnerability:** `LinterConfig::from_file` used `fs::metadata` followed by `fs::read_to_string`, which caused a TOCTOU (Time of Check, Time of Use) issue. During the race window, a file could be swapped to an excessively large file which bypasses the size check, leading to an unbounded read and OOM DoS.
**Learning:** `fs::read_to_string` combined with `metadata()` is unsafe against TOCTOU and OOM issues because it relies on the state of the filesystem that can change between checking and reading.
**Prevention:** Use `File::open` to hold the file descriptor, then `Read::take(MAX_CONFIG_SIZE + 1)` and `read_to_string` to securely bound file reads and prevent memory exhaustion.
