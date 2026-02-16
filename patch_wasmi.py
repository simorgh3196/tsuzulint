import re

file_path = 'crates/tsuzulint_plugin/src/executor_wasmi.rs'

with open(file_path, 'r') as f:
    content = f.read()

# Replace read_string with read_bytes
content = content.replace(
    'fn read_string(store: &Store<HostState>, ptr: i32, len: i32) -> Result<String, PluginError> {',
    'fn read_bytes(store: &Store<HostState>, ptr: i32, len: i32) -> Result<Vec<u8>, PluginError> {'
)
content = content.replace(
    'String::from_utf8(data.to_vec())\n            .map_err(|e| PluginError::call(format!("Invalid UTF-8: {}", e)))',
    'Ok(data.to_vec())'
)

# Replace write_string with write_bytes
content = content.replace(
    'fn write_string(\n        store: &mut Store<HostState>,\n        alloc_fn: &TypedFunc<i32, i32>,\n        data: &str,\n    ) -> Result<(i32, i32), PluginError> {',
    'fn write_bytes(\n        store: &mut Store<HostState>,\n        alloc_fn: &TypedFunc<i32, i32>,\n        data: &[u8],\n    ) -> Result<(i32, i32), PluginError> {'
)
content = content.replace(
    'let bytes = data.as_bytes();',
    'let bytes = data;'
)

# Replace call_lint implementation
old_call_lint = r'''    fn call_lint(&mut self, rule_name: &str, input_json: &str) -> Result<String, PluginError> {
        let rule = self
            .rules
            .get_mut(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;

        // Reset fuel before execution
        rule.store
            .set_fuel(DEFAULT_FUEL_LIMIT)
            .map_err(|e| PluginError::call(format!("Failed to set fuel: {}", e)))?;

        // Write input to WASM memory
        let (input_ptr, input_len) =
            Self::write_string(&mut rule.store, &rule.alloc_fn, input_json)?;

        // Call lint function
        let (output_ptr, output_len) =
            rule.lint_fn
                .call(&mut rule.store, (input_ptr, input_len))
                .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;

        // Read output from WASM memory
        Self::read_string(&rule.store, output_ptr, output_len)
    }'''

new_call_lint = r'''    fn call_lint(&mut self, rule_name: &str, input_bytes: &[u8]) -> Result<Vec<u8>, PluginError> {
        let rule = self
            .rules
            .get_mut(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;

        // Reset fuel before execution
        rule.store
            .set_fuel(DEFAULT_FUEL_LIMIT)
            .map_err(|e| PluginError::call(format!("Failed to set fuel: {}", e)))?;

        // Write input to WASM memory
        let (input_ptr, input_len) =
            Self::write_bytes(&mut rule.store, &rule.alloc_fn, input_bytes)?;

        // Call lint function
        let (output_ptr, output_len) =
            rule.lint_fn
                .call(&mut rule.store, (input_ptr, input_len))
                .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;

        // Read output from WASM memory
        Self::read_bytes(&rule.store, output_ptr, output_len)
    }'''

# Since regex is risky, I'll try to find unique parts of call_lint to replace
if 'fn call_lint(&mut self, rule_name: &str, input_json: &str) -> Result<String, PluginError> {' in content:
    # We need to replace the whole function body carefully.
    # The read_string call is the last statement.
    content = content.replace(
        'fn call_lint(&mut self, rule_name: &str, input_json: &str) -> Result<String, PluginError> {',
        'fn call_lint(&mut self, rule_name: &str, input_bytes: &[u8]) -> Result<Vec<u8>, PluginError> {'
    )
    content = content.replace(
        'Self::write_string(&mut rule.store, &rule.alloc_fn, input_json)?;',
        'Self::write_bytes(&mut rule.store, &rule.alloc_fn, input_bytes)?;'
    )
    content = content.replace(
        'Self::read_string(&rule.store, output_ptr, output_len)',
        'Self::read_bytes(&rule.store, output_ptr, output_len)'
    )

# Also update get_manifest usage in load() to use read_bytes and convert to string (since manifest is still JSON)
# let manifest_json = Self::read_string(&store, manifest_ptr, manifest_len)?;
content = content.replace(
    'let manifest_json = Self::read_string(&store, manifest_ptr, manifest_len)?;',
    'let manifest_bytes = Self::read_bytes(&store, manifest_ptr, manifest_len)?;\n        let manifest_json = String::from_utf8(manifest_bytes).map_err(|e| PluginError::invalid_manifest(format!("Invalid UTF-8 in manifest: {}", e)))?;'
)

# Update tests
content = content.replace('executor.call_lint("error-rule", "{}")', 'executor.call_lint("error-rule", b"{}")')
content = content.replace('executor.call_lint("test-rule", &input_json)', 'executor.call_lint("test-rule", input_json.as_bytes())')
content = content.replace('executor.call_lint("invalid-utf8", "{}")', 'executor.call_lint("invalid-utf8", b"{}")')
content = content.replace('executor.call_lint("infinite-loop", "{}")', 'executor.call_lint("infinite-loop", b"{}")')

# Update test assertions that check result.unwrap() which is now Vec<u8>
# assert!(result.unwrap().contains("\u{FFFD}")); -> Check for replacement char bytes?
# The test_read_string_invalid_utf8 expects string error or replacement.
# If read_bytes returns raw bytes, it won't error on invalid UTF-8.
# So test_read_string_invalid_utf8 logic needs to change or be removed since read_bytes is safe.
# We can remove the test or change it to verify raw bytes are returned.

# Let's remove test_read_string_invalid_utf8 body logic or adjust it.
# The WASM returns \ff\fe at offset 100.
# assert!(result.is_err() || result.unwrap().contains("\u{FFFD}"));
# New result is Vec<u8>. It will be Ok(vec![0xff, 0xfe]).
# So assertion: assert_eq!(result.unwrap(), vec![0xff, 0xfe]);

test_utf8_code = r'''        let result = executor.call_lint("invalid-utf8", "{}");
        // Should handle the UTF-8 error (may succeed with replacement chars or fail gracefully)
        // The exact behavior depends on String::from_utf8_lossy vs from_utf8
        // Our implementation uses from_utf8, so it should error
        assert!(result.is_err() || result.unwrap().contains("\u{FFFD}"));'''

new_test_utf8_code = r'''        let result = executor.call_lint("invalid-utf8", b"{}");
        // read_bytes should return raw bytes without UTF-8 validation
        assert_eq!(result.unwrap(), vec![0xff, 0xfe]);'''

content = content.replace(test_utf8_code, new_test_utf8_code)
# Also fix the call in the replaced block above
content = content.replace('executor.call_lint("invalid-utf8", b"{}")', 'executor.call_lint("invalid-utf8", b"{}")')

with open(file_path, 'w') as f:
    f.write(content)
