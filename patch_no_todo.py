import re

file_path = 'rules/no-todo/src/lib.rs'

with open(file_path, 'r') as f:
    content = f.read()

# Update lint signature
content = content.replace(
    'pub fn lint(input: String) -> FnResult<String> {',
    'pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>> {'
)

# Update lint_impl signature
content = content.replace(
    'fn lint_impl(input: String) -> FnResult<String> {',
    'fn lint_impl(input: Vec<u8>) -> FnResult<Vec<u8>> {'
)

# Update lint_impl body to use rmp_serde
content = content.replace(
    'let request: LintRequest = serde_json::from_str(&input)?;',
    'let request: LintRequest = rmp_serde::from_slice(&input)?;'
)

content = content.replace(
    'return Ok(serde_json::to_string(&LintResponse { diagnostics })?);',
    'return Ok(rmp_serde::to_vec(&LintResponse { diagnostics })?);'
)

content = content.replace(
    'Ok(serde_json::to_string(&LintResponse { diagnostics })?)',
    'Ok(rmp_serde::to_vec(&LintResponse { diagnostics })?)'
)

# Update tests
# create_request
content = content.replace(
    'fn create_request(text: &str, config: serde_json::Value) -> String {',
    'fn create_request(text: &str, config: serde_json::Value) -> Vec<u8> {'
)
content = content.replace(
    'serde_json::to_string(&request).unwrap()',
    'rmp_serde::to_vec(&request).unwrap()'
)

# parse_response
content = content.replace(
    'fn parse_response(json: &str) -> LintResponse {',
    'fn parse_response(bytes: &[u8]) -> LintResponse {'
)
content = content.replace(
    'serde_json::from_str(json).unwrap()',
    'rmp_serde::from_slice(bytes).unwrap()'
)

with open(file_path, 'w') as f:
    f.write(content)
