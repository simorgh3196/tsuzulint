import re

file_path = 'crates/tsuzulint_plugin/src/executor_wasmi.rs'

with open(file_path, 'r') as f:
    content = f.read()

# Fix 1: test_executor_large_input call_lint
content = content.replace(
    'let result = executor.call_lint("test-rule", &input_json);',
    'let result = executor.call_lint("test-rule", input_json.as_bytes());'
)

# Fix 3: test_read_string_invalid_utf8
# Using raw string for search pattern to avoid escape issues
search_pat = r'assert!(result.is_err() || result.unwrap().contains("\u{FFFD}"));'
replace_pat = r'assert_eq!(result.unwrap(), vec![0xff, 0xfe]);'
content = content.replace(search_pat, replace_pat)

content = content.replace(
    'executor.call_lint("test-rule", "{\"text\":\"hello\"}")',
    'executor.call_lint("test-rule", b"{\"text\":\"hello\"}")'
)

content = content.replace(
    'assert_eq!(result.unwrap(), "[]");',
    'assert_eq!(result.unwrap(), b"[]");'
)

content = content.replace(
    'executor.call_lint("nonexistent", "{}")',
    'executor.call_lint("nonexistent", b"{}")'
)

with open(file_path, 'w') as f:
    f.write(content)
