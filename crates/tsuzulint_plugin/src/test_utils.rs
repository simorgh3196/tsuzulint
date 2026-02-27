//! Test utilities for tsuzulint_plugin.

/// Helper to compile WAT to WASM bytes
pub fn wat_to_wasm(wat_source: &str) -> Vec<u8> {
    wat::parse_str(wat_source).expect("Invalid WAT")
}

/// Helper to create a basic valid rule in WAT for testing.
pub fn valid_rule_wat() -> String {
    let json = r#"{"name":"test-rule","version":"1.0.0","description":"Test rule"}"#;
    let len = json.len();
    format!(
        r#"
        (module
            (memory (export "memory") 1)
            (func (export "get_manifest") (result i32 i32)
                (i32.const 0) ;; ptr
                (i32.const {}) ;; len
            )
            (func (export "lint") (param i32 i32) (result i32 i32)
                (i32.const 100) ;; ptr to "[]" (move past manifest)
                (i32.const 2)   ;; len
            )
            (func (export "alloc") (param i32) (result i32)
                (i32.const 128) ;; return a fixed pointer
            )
            ;; Write manifest to memory at offset 0
            (data (i32.const 0) "{}")
            ;; Write empty array to memory at offset 100
            (data (i32.const 100) "[]")
        )
        "#,
        len,
        json.replace("\"", "\\\"") // Escape quotes for WAT string
    )
}
