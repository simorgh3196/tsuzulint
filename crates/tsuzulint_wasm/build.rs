use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../tsuzulint_core/tests/fixtures/simple_rule/src/lib.rs");
    println!("cargo:rerun-if-changed=../tsuzulint_core/tests/fixtures/simple_rule/Cargo.toml");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let fixture_dir = manifest_dir.join("../tsuzulint_core/tests/fixtures/simple_rule");

    let status = Command::new("cargo")
        .args(["build", "--target", "wasm32-wasip1", "--release"])
        .current_dir(&fixture_dir)
        .status()
        .expect("failed to execute cargo build for simple_rule fixture");

    if !status.success() {
        panic!("failed to build simple_rule fixture");
    }
}
