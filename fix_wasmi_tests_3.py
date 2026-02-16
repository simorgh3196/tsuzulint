import re

file_path = 'crates/tsuzulint_plugin/src/executor_wasmi.rs'

with open(file_path, 'r') as f:
    content = f.read()

# Replace call_lint string arg with byte arg
# Be careful not to double prefix if already fixed
content = content.replace(
    'executor.call_lint("test-rule", "',
    'executor.call_lint("test-rule", b"'
)

# Fix back if we accidentally made bb"..."
content = content.replace('bb"', 'b"')

with open(file_path, 'w') as f:
    f.write(content)
