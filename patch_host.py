import re

file_path = 'crates/tsuzulint_plugin/src/host.rs'

with open(file_path, 'r') as f:
    content = f.read()

# Update LintRequest struct
# Old:
#     #[serde(borrow)]
#     source: &'a serde_json::value::RawValue,
# New:
#     source: String,

content = content.replace(
    '    #[serde(borrow)]\n    source: &\'a serde_json::value::RawValue,',
    '    source: String,'
)

# Update run_rule_with_parts implementation
# Old:
#         let request = LintRequest {
#             node,
#             config,
#             source,
#             file_path,
#         };
#
#         let real_name = aliases.get(name).map(|s| s.as_str()).unwrap_or(name);
#
#         let request_json = serde_json::to_string(&request)?;
#         let response_json = executor.call_lint(real_name, &request_json)?;
#
#         let response: LintResponse = serde_json::from_str(&response_json)
#             .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;

# New:
#         // Parse source JSON string to String for Msgpack
#         let source_str: String = serde_json::from_str(source.get())
#             .map_err(|e| PluginError::call(format!("Invalid source JSON: {}", e)))?;
#
#         let request = LintRequest {
#             node,
#             config,
#             source: source_str,
#             file_path,
#         };
#
#         let real_name = aliases.get(name).map(|s| s.as_str()).unwrap_or(name);
#
#         // Use Msgpack serialization
#         let request_bytes = rmp_serde::to_vec(&request)
#             .map_err(|e| PluginError::call(format!("Failed to serialize request: {}", e)))?;
#
#         let response_bytes = executor.call_lint(real_name, &request_bytes)?;
#
#         let response: LintResponse = rmp_serde::from_slice(&response_bytes)
#             .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;

# Since regex is safer for block replacement
block_pattern = r'        let request = LintRequest \{\s*node,\s*config,\s*source,\s*file_path,\s*\};\s*let real_name = aliases\.get\(name\)\.map\(\|s\| s\.as_str\(\)\)\.unwrap_or\(name\);\s*let request_json = serde_json::to_string\(&request\)\?;\s*let response_json = executor\.call_lint\(real_name, &request_json\)\?;\s*let response: LintResponse = serde_json::from_str\(&response_json\)\s*\.map_err\(\|e\| PluginError::call\(format!\("Invalid response from \'{}\': {}", name, e\)\)\)\?;'

# The regex above is fragile due to whitespace.
# I'll use string replacement if possible, assuming indentation matches.

old_block = r'''        let request = LintRequest {
            node,
            config,
            source,
            file_path,
        };

        let real_name = aliases.get(name).map(|s| s.as_str()).unwrap_or(name);

        let request_json = serde_json::to_string(&request)?;
        let response_json = executor.call_lint(real_name, &request_json)?;

        let response: LintResponse = serde_json::from_str(&response_json)
            .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;'''

new_block = r'''        let source_str: String = serde_json::from_str(source.get())
            .map_err(|e| PluginError::call(format!("Invalid source JSON: {}", e)))?;

        let request = LintRequest {
            node,
            config,
            source: source_str,
            file_path,
        };

        let real_name = aliases.get(name).map(|s| s.as_str()).unwrap_or(name);

        let request_bytes = rmp_serde::to_vec(&request)
             .map_err(|e| PluginError::call(format!("Failed to serialize request: {}", e)))?;

        let response_bytes = executor.call_lint(real_name, &request_bytes)?;

        let response: LintResponse = rmp_serde::from_slice(&response_bytes)
            .map_err(|e| PluginError::call(format!("Invalid response from '{}': {}", name, e)))?;'''

if old_block in content:
    content = content.replace(old_block, new_block)
else:
    # Try finding approximate location via index
    start_idx = content.find('let request = LintRequest {')
    end_idx = content.find('Ok(response.diagnostics)')
    if start_idx != -1 and end_idx != -1:
         # Found block boundary
         content = content[:start_idx] + new_block + "\n\n        " + content[end_idx:]

with open(file_path, 'w') as f:
    f.write(content)
