//! Plugin resolution logic.

use std::path::{Path, PathBuf};

/// Resolves plugin paths from names.
pub struct PluginResolver;

impl PluginResolver {
    /// Resolves a plugin name to a filesystem path.
    ///
    /// Search order:
    /// 1. `$PROJECT_ROOT/.tsuzulint/plugins/<name>.wasm`
    /// 2. `$HOME/.tsuzulint/plugins/<name>.wasm`
    pub fn resolve(name: &str, project_root: Option<&Path>) -> Option<PathBuf> {
        // Validate plugin name to prevent path traversal
        let path = Path::new(name);
        let mut components = path.components();
        match (components.next(), components.next()) {
            (Some(std::path::Component::Normal(_)), None) => {
                // Good: exactly one normal component
            }
            _ => return None,
        }

        let filename = format!("{}.wasm", name);

        // 1. Check local project directory
        if let Some(root) = project_root {
            let local_path = root.join(".tsuzulint").join("plugins").join(&filename);
            if local_path.is_file() {
                return Some(local_path);
            }
        }

        // 2. Check global home directory
        if let Some(home) = dirs::home_dir() {
            let global_path = home.join(".tsuzulint").join("plugins").join(&filename);
            if global_path.is_file() {
                return Some(global_path);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_local() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        let plugins_dir = project_root.join(".tsuzulint").join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let plugin_path = plugins_dir.join("my-rule.wasm");
        fs::write(&plugin_path, "").unwrap();

        let resolved = PluginResolver::resolve("my-rule", Some(project_root));
        assert_eq!(resolved, Some(plugin_path));
    }

    #[test]
    fn test_resolve_not_found() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();

        let resolved = PluginResolver::resolve("non-existent", Some(project_root));
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_resolve_invalid_names() {
        assert_eq!(PluginResolver::resolve("../plugin", None), None);
        assert_eq!(PluginResolver::resolve("/plugin", None), None);
        assert_eq!(PluginResolver::resolve("dir/plugin", None), None);
        assert_eq!(PluginResolver::resolve(".", None), None);
        assert_eq!(PluginResolver::resolve("..", None), None);
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_trailing_separator() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();

        // Should return None, even if it passes validation
        assert_eq!(PluginResolver::resolve("foo/", Some(project_root)), None);
    }
}
