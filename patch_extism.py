import re

with open('crates/tsuzulint_plugin/src/executor_extism.rs', 'r') as f:
    content = f.read()

# Replace call_lint implementation
new_impl = r"""    fn call_lint(&mut self, rule_name: &str, input_bytes: &[u8]) -> Result<Vec<u8>, PluginError> {
        let rule = self
            .rules
            .get_mut(rule_name)
            .ok_or_else(|| PluginError::not_found(rule_name))?;

        let response_bytes = rule
            .plugin
            .call::<&[u8], &[u8]>("lint", input_bytes)
            .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;

        Ok(response_bytes.to_vec())
    }"""

# Regex to find existing call_lint implementation
# It starts with fn call_lint and ends with } (nested braces handling is tricky with regex, but here indentation helps)
# The implementation is:
#     fn call_lint(&mut self, rule_name: &str, input_json: &str) -> Result<String, PluginError> {
#         let rule = self
#             .rules
#             .get_mut(rule_name)
#             .ok_or_else(|| PluginError::not_found(rule_name))?;
#
#         let response_json: String = rule
#             .plugin
#             .call("lint", input_json)
#             .map_err(|e| PluginError::call(format!("Rule '{}' failed: {}", rule_name, e)))?;
#
#         Ok(response_json)
#     }

pattern = r'fn call_lint\(&mut self, rule_name: &str, input_json: &str\) -> Result<String, PluginError> \s*\{[^}]*call\("lint", input_json\)[^}]*Ok\(response_json\)\s*\}'

# Since regex is hard for multiline with nested stuff, let's use string replacement if exact match
old_impl_start = 'fn call_lint(&mut self, rule_name: &str, input_json: &str) -> Result<String, PluginError> {'
if old_impl_start in content:
    # Find the end of the function block manually or just replace the known body
    start_idx = content.find(old_impl_start)
    # Assume 4 space indentation for the function body end
    # We can scan forward until we find the closing brace at the same indentation level (4 spaces)
    # But simpler is to find the next function 'fn unload' or end of impl block
    end_marker = '    fn unload(&mut self'
    end_idx = content.find(end_marker)

    if start_idx != -1 and end_idx != -1:
        # Extract the old body to be sure
        old_body = content[start_idx:end_idx]
        # Replace
        content = content[:start_idx] + new_impl + "\n\n" + content[end_idx:]

# Also update the test
test_call = 'let result = executor.call_lint("nonexistent", "{}");'
new_test_call = 'let result = executor.call_lint("nonexistent", &[]);'
content = content.replace(test_call, new_test_call)

with open('crates/tsuzulint_plugin/src/executor_extism.rs', 'w') as f:
    f.write(content)
