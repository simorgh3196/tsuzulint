#[cfg(test)]
mod tests {
    use std::fs::File;
    use tempfile::tempdir;
    use tsuzulint_core::rule_manifest::load_rule_manifest;

    #[test]
    fn test_load_rule_manifest_open_error() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("tsuzulint-rule.json");
        File::create(&manifest_path).unwrap();

        let mut perms = std::fs::metadata(&manifest_path).unwrap().permissions();
        perms.set_readonly(true);
        // On Unix, we need to remove read permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o000);
        }
        std::fs::set_permissions(&manifest_path, perms).unwrap();

        let result = load_rule_manifest(&manifest_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        println!("Error: {}", err);
        assert!(err.contains("Failed to open") || err.contains("Permission denied") || err.contains("Failed to read metadata"));
    }
}
