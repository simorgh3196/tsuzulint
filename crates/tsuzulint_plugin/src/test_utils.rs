//! Test utilities for tsuzulint_plugin.

use crate::RuleManifest;

/// Helper to compile WAT to WASM bytes
pub fn wat_to_wasm(wat_source: &str) -> Vec<u8> {
    wat::parse_str(wat_source).expect("Invalid WAT")
}

/// Serialize a RuleManifest to MsgPack bytes.
pub fn manifest_to_msgpack(manifest: &RuleManifest) -> Vec<u8> {
    rmp_serde::to_vec_named(manifest).expect("Failed to serialize manifest to MsgPack")
}

/// Encode bytes as a WAT data string (each byte as `\xx`).
pub fn bytes_to_wat_data(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{:02x}", b)).collect()
}

/// Helper to create a basic valid rule in WAT for testing.
pub fn valid_rule_wat() -> String {
    let manifest =
        RuleManifest::new("test-rule", "1.0.0").with_description("Test rule".to_string());
    let msgpack_bytes = manifest_to_msgpack(&manifest);
    let len = msgpack_bytes.len();
    let data = bytes_to_wat_data(&msgpack_bytes);
    format!(
        r#"
        (module
            (memory (export "memory") 1)
            (func (export "get_manifest") (result i32 i32)
                (i32.const 0) ;; ptr
                (i32.const {len}) ;; len
            )
            (func (export "lint") (param i32 i32) (result i32 i32)
                (i32.const 512) ;; ptr to "[]" (well past manifest)
                (i32.const 2)   ;; len
            )
            (func (export "alloc") (param i32) (result i32)
                (i32.const 1024) ;; return a fixed pointer
            )
            ;; Write MsgPack manifest to memory at offset 0
            (data (i32.const 0) "{data}")
            ;; Write empty array to memory at offset 512
            (data (i32.const 512) "[]")
        )
        "#,
    )
}
