use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, Once};

static BUILD_ONCE: Once = Once::new();
static BUILD_SUCCESS: Mutex<bool> = Mutex::new(false);

/// Builds the simple_rule WASM fixture and returns the path to the WASM file.
/// Returns None if the build fails (e.g. missing target).
pub fn build_simple_rule_wasm() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_dir = manifest_dir.join("tests/fixtures/simple_rule");
    let target_dir = fixture_dir.join("target");
    let wasm_path = target_dir.join("wasm32-wasip1/release/simple_rule.wasm");

    BUILD_ONCE.call_once(|| {
        println!("Building WASM fixture in {}", fixture_dir.display());
        let status = Command::new("cargo")
            .args(["build", "--target", "wasm32-wasip1", "--release"])
            .current_dir(&fixture_dir)
            .status();

        match status {
            Ok(s) if s.success() => {
                *BUILD_SUCCESS.lock().unwrap() = true;
            }
            Ok(s) => {
                eprintln!("WASM fixture build failed with status: {}", s);
            }
            Err(e) => {
                eprintln!("Failed to execute cargo build for WASM fixture: {}", e);
            }
        }
    });

    if *BUILD_SUCCESS.lock().unwrap() && wasm_path.exists() {
        Some(wasm_path)
    } else {
        None
    }
}
