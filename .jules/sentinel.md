# Sentinel Changelog

## 2026-02-13 - [Fix Arbitrary File Read via Absolute Paths]

**Vulnerability:** The linter configuration allowed loading rule manifests from absolute paths. A malicious configuration file could point to sensitive system files (e.g., `/etc/passwd`), causing the linter to read them and attempt to parse them as JSON. This could lead to Arbitrary File Read (if error messages leak content) or Denial of Service (reading large/special files).

**Learning:** `Path::join` replaces the base path if the joined path is absolute. This behavior is standard in Rust but dangerous when joining untrusted input to a base directory. Always validate that user-provided paths are relative before joining.

**Prevention:** I implemented `Linter::resolve_manifest_path` which strictly validates that the rule path is relative (`is_absolute()`, `has_root()`), does not contain directory traversal components (`..`), and resolves within the base directory (preventing symlink traversal).
