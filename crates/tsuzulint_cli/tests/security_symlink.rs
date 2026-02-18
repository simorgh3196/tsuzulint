use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_file as symlink;

#[test]
fn test_init_symlink_overwrite_vulnerability() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".tsuzulint.jsonc");
    let target_path = dir.path().join("target_file");

    // Create target file with content
    fs::write(&target_path, "Important Data").unwrap();

    // Create symlink: config -> target
    #[cfg(unix)]
    symlink(&target_path, &config_path).unwrap();
    #[cfg(windows)]
    // On Windows, creating symlinks might require privileges.
    // If it fails, we might skip the test or use a hard link if relevant,
    // but hard links behave differently for overwrites.
    // For this security test, we assume we can create symlinks (e.g. Developer Mode).
    // If not, we skip.
    if symlink(&target_path, &config_path).is_err() {
        return;
    }

    // Run `tzlint init --force`
    let mut cmd = cargo_bin_cmd!("tzlint");
    cmd.current_dir(dir.path()).arg("init").arg("--force");

    // Before fix: This overwrites the target file through the symlink.
    // After fix: This should replace the symlink with a file, preserving the target.

    cmd.assert().success();

    // Check if target file was modified
    let content = fs::read_to_string(&target_path).unwrap();

    // NOTE: This assertion WILL FAIL before the fix is applied.
    // We expect "Important Data" to be preserved.
    assert_eq!(
        content, "Important Data",
        "Security vulnerability: Target file was overwritten!"
    );

    // Check if config file is now a regular file (not a symlink)
    let meta = fs::symlink_metadata(&config_path).unwrap();
    assert!(meta.is_file(), "Config file should be a regular file now");
    assert!(
        !meta.is_symlink(),
        "Config file should not be a symlink anymore"
    );
}

#[test]
fn test_init_without_force_rejects_existing_symlink() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".tsuzulint.jsonc");
    let target_path = dir.path().join("target_file");

    fs::write(&target_path, r#"{ "rules": [] }"#).unwrap();

    #[cfg(unix)]
    symlink(&target_path, &config_path).unwrap();
    #[cfg(windows)]
    if symlink(&target_path, &config_path).is_err() {
        return;
    }

    let mut cmd = cargo_bin_cmd!("tzlint");
    cmd.current_dir(dir.path()).arg("init");

    cmd.assert().failure();

    let content = fs::read_to_string(&target_path).unwrap();
    assert_eq!(content, r#"{ "rules": [] }"#);
}
