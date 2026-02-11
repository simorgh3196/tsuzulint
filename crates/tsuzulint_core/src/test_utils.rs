use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

static BUILD_WASM: Once = Once::new();

/// Builds the simple_rule WASM fixture and returns the path to the WASM file.
pub fn build_simple_rule_wasm() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_dir = manifest_dir.join("tests/fixtures/simple_rule");
    let target_dir = fixture_dir.join("target");
    let wasm_path = target_dir
        .join("wasm32-wasip1/release/simple_rule.wasm");

    BUILD_WASM.call_once(|| {
        println!("Building WASM fixture in {}", fixture_dir.display());
        let status = Command::new("cargo")
            .args(&["build", "--target", "wasm32-wasip1", "--release"])
            .current_dir(&fixture_dir)
            .status()
            .expect("Failed to execute cargo build for WASM fixture");

        if !status.success() {
            panic!("Failed to build WASM fixture");
        }
    });

    if !wasm_path.exists() {
        panic!("WASM file not found at expected path: {}", wasm_path.display());
    }

    wasm_path
}
