import re
import os

files = [
    'rules/sentence-length/src/lib.rs',
    'rules/no-doubled-joshi/src/lib.rs'
]

for file_path in files:
    if not os.path.exists(file_path):
        continue

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
    # test_lint_simple in sentence-length
    if 'test_lint_simple' in content:
        # Replace lint_impl(request.to_string()) with rmp_serde::to_vec
        # This is hard with simple replace because request is a variable.
        # Pattern: lint_impl(request.to_string()).unwrap()

        # We need to construct bytes from request (which is serde_json::Value in test)
        # request is created with serde_json::json!({...})
        # rmp_serde::to_vec(&request).unwrap()

        content = content.replace(
            'lint_impl(request.to_string()).unwrap()',
            'lint_impl(rmp_serde::to_vec(&request).unwrap()).unwrap()'
        )

        content = content.replace(
            'let response: LintResponse = serde_json::from_str(&output).unwrap();',
            'let response: LintResponse = rmp_serde::from_slice(&output).unwrap();'
        )

    # Check for no-doubled-joshi tests
    if 'no-doubled-joshi' in file_path:
        # It likely has similar tests. Let's look for lint_impl calls.
        content = content.replace(
            'lint_impl(request.to_string()).unwrap()',
            'lint_impl(rmp_serde::to_vec(&request).unwrap()).unwrap()'
        )
        content = content.replace(
            'let response: LintResponse = serde_json::from_str(&output).unwrap();',
            'let response: LintResponse = rmp_serde::from_slice(&output).unwrap();'
        )

        # Also check create_request usage if any (copied from no-todo)
        content = content.replace(
            'fn create_request(text: &str, config: serde_json::Value) -> String {',
            'fn create_request(text: &str, config: serde_json::Value) -> Vec<u8> {'
        )
        content = content.replace(
            'serde_json::to_string(&request).unwrap()',
            'rmp_serde::to_vec(&request).unwrap()'
        )
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
