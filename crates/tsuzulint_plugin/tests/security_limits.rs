#[cfg(feature = "native")]
mod native_tests {
    use tsuzulint_plugin::PluginHost;

    /// Helper to compile WAT to WASM bytes
    fn wat_to_wasm(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).expect("Invalid WAT")
    }

    #[test]
    fn test_plugin_host_memory_limit_breach() {
        let mut host = PluginHost::new();

        // A rule that tries to allocate 512MB (8192 pages)
        // This should fail if we had a limit (e.g. 128MB = 2048 pages)
        // We use Extism ABI for imports to avoid ABI mismatch errors
        let wasm = wat_to_wasm(
            r#"
            (module
                (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
                (memory (export "memory") 8192) ;; 8192 pages * 64KB = 512MB

                ;; Manifest JSON at offset 0
                (data (i32.const 0) "{\"name\":\"dos-rule\",\"version\":\"1.0.0\"}")

                (func (export "get_manifest")
                    (call $output_set (i64.const 0) (i64.const 37))
                )

                (func (export "lint")
                    (call $output_set (i64.const 0) (i64.const 0))
                )

                (func (export "alloc") (param i64) (result i64)
                    (i64.const 0)
                )
            )
            "#,
        );

        // If this succeeds, it confirms we don't have strict memory limits
        let result = host.load_rule_bytes(&wasm);

        // Assert that it FAILS with OOM or resource limit error
        match result {
            Err(tsuzulint_plugin::PluginError::CallError(msg))
                if msg.to_lowercase().contains("oom") || msg.to_lowercase().contains("memory") => {
                // Success! The limit worked.
            }
             Err(tsuzulint_plugin::PluginError::LoadError(msg))
                if msg.to_lowercase().contains("limit") || msg.to_lowercase().contains("memory") => {
                 // Success! The limit worked at load time.
             }
            Ok(_) => panic!("Expected OOM error, but load succeeded! Memory limits are not enforced."),
            Err(e) => panic!("Expected OOM error, but got unexpected error: {:?}", e),
        }
    }

}
