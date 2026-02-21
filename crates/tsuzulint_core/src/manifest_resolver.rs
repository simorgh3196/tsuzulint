//! Manifest path resolution with security checks.

use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Resolves the manifest path, ensuring security constraints.
///
/// # Security
///
/// This function enforces the following security constraints:
/// - Rejects absolute paths
/// - Rejects paths containing `..` (directory traversal)
/// - Ensures resolved path stays within base directory (symlink-safe)
pub fn resolve_manifest_path(base_dir: Option<&Path>, path: &str) -> Option<PathBuf> {
    let p = Path::new(path);

    if p.is_absolute() || p.has_root() {
        warn!("Ignoring absolute rule path: {}", path);
        return None;
    }

    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        warn!("Ignoring rule path containing '..': {}", path);
        return None;
    }

    if let Some(base) = base_dir {
        let joined = base.join(path);
        match (joined.canonicalize(), base.canonicalize()) {
            (Ok(canon_path), Ok(canon_base)) => {
                if canon_path.starts_with(&canon_base) {
                    Some(canon_path)
                } else {
                    warn!(
                        "Ignoring rule path that resolves outside base directory: {}",
                        path
                    );
                    None
                }
            }
            (Err(e), _) => {
                debug!(
                    "Failed to canonicalize rule path '{}': {}",
                    joined.display(),
                    e
                );
                None
            }
            (_, Err(e)) => {
                warn!("Failed to canonicalize base directory: {}", e);
                None
            }
        }
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_manifest_path_relative() {
        let temp_dir = tempdir().unwrap();
        let base = temp_dir.path();
        let path = "rule.json";
        fs::write(base.join(path), "").unwrap();

        let resolved = resolve_manifest_path(Some(base), path);
        assert_eq!(resolved, Some(base.join(path).canonicalize().unwrap()));
    }

    #[test]
    fn test_resolve_manifest_path_traversal_rejected() {
        let base = Path::new("/tmp/base");
        let path = "../../etc/passwd";

        let resolved = resolve_manifest_path(Some(base), path);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_resolve_manifest_path_outside_base_rejected() {
        let temp_dir = tempdir().unwrap();
        let base = temp_dir.path().join("base");
        fs::create_dir(&base).unwrap();

        let outside_file = temp_dir.path().join("outside.json");
        fs::write(&outside_file, "").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link_path = base.join("link.json");
            symlink(&outside_file, &link_path).unwrap();

            let resolved = resolve_manifest_path(Some(&base), "link.json");
            assert_eq!(resolved, None);
        }
    }

    #[test]
    fn test_resolve_manifest_path_no_base_dir() {
        let resolved = resolve_manifest_path(None, "rule.json");
        assert_eq!(resolved, Some(PathBuf::from("rule.json")));
    }

    #[test]
    fn test_resolve_manifest_path_absolute_rejected() {
        #[cfg(unix)]
        let abs_path = "/etc/passwd";
        #[cfg(windows)]
        let abs_path = r"C:\Windows\System32\drivers\etc\hosts";
        #[cfg(not(any(unix, windows)))]
        let abs_path = "/absolute/path";

        let base = Path::new("/tmp/base");
        let resolved = resolve_manifest_path(Some(base), abs_path);
        assert_eq!(resolved, None);
    }
}
