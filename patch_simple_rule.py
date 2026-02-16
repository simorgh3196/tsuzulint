import re

file_path = 'crates/tsuzulint_core/tests/fixtures/simple_rule/src/lib.rs'

with open(file_path, 'r') as f:
    content = f.read()

# Update lint signature
content = content.replace(
    'pub fn lint(input: String) -> FnResult<String> {',
    'pub fn lint(input: Vec<u8>) -> FnResult<Vec<u8>> {'
)

# Update lint body
content = content.replace(
    'let request: LintRequest = serde_json::from_str(&input)?;',
    'let request: LintRequest = rmp_serde::from_slice(&input)?;'
)

content = content.replace(
    'Ok(serde_json::to_string(&LintResponse { diagnostics })?)',
    'Ok(rmp_serde::to_vec(&LintResponse { diagnostics })?)'
)

with open(file_path, 'w') as f:
    f.write(content)
