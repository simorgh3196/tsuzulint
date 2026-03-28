use serde_json::json;
use tempfile::NamedTempFile;
use tsuzulint_registry::{PluginResolver, PluginSource, PluginSpec};

const MAX_WASM_SIZE: u64 = 50 * 1024 * 1024;

#[tokio::test]
async fn test_resolve_local_exceeds_max_wasm_size() {
    let resolver = PluginResolver::new().unwrap();
    let dir = tempfile::tempdir().unwrap();

    // Create oversized WASM
    let wasm_file = NamedTempFile::new_in(&dir).unwrap();
    wasm_file.as_file().set_len(MAX_WASM_SIZE + 1024).unwrap();

    // Create valid manifest pointing to oversized WASM
    let manifest_path = dir.path().join("tsuzulint-rule.json");
    let manifest = json!({
        "rule": { "name": "oversized-rule", "version": "1.0.0" },
        "wasm": [{
            "path": wasm_file.path().file_name().unwrap().to_str().unwrap(),
            "hash": "0000000000000000000000000000000000000000000000000000000000000000"
        }]
    });
    std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

    let spec = PluginSpec {
        source: PluginSource::Path(manifest_path.clone()),
        alias: None,
    };

    let result = resolver.resolve(&spec).await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("WASM file too large") || err_str.contains("too large"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_resolve_local_fifo_exceeds_max_wasm_size() {
    let resolver = PluginResolver::new().unwrap();
    let dir = tempfile::tempdir().unwrap();

    let wasm_path = dir.path().join("fifo.wasm");
    let status = std::process::Command::new("mkfifo")
        .arg(&wasm_path)
        .status();

    if status.is_err() || !status.unwrap().success() {
        return;
    }

    let manifest_path = dir.path().join("tsuzulint-rule.json");
    let manifest = json!({
        "rule": { "name": "oversized-rule", "version": "1.0.0" },
        "wasm": [{
            "path": "fifo.wasm",
            "hash": "0000000000000000000000000000000000000000000000000000000000000000"
        }]
    });
    std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

    let spec = PluginSpec {
        source: PluginSource::Path(manifest_path.clone()),
        alias: None,
    };

    let wasm_path_clone = wasm_path.clone();
    let handle = std::thread::spawn(move || {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .write(true)
            .open(&wasm_path_clone)
        {
            use std::io::Write;
            let chunk = vec![0u8; 1024 * 1024]; // 1MB chunk
            for _ in 0..51 {
                let _ = file.write_all(&chunk);
            }
        }
    });

    let result = resolver.resolve(&spec).await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("exceeded max size during read")
            || err_str.contains("WASM file exceeded max size during read")
    );

    handle.join().unwrap();
}
