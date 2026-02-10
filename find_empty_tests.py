import os
import re

for root, dirs, files in os.walk("."):
    if "target" in dirs:
        dirs.remove("target")
    if "node_modules" in dirs:
        dirs.remove("node_modules")

    for file in files:
        if file.endswith(".rs"):
            path = os.path.join(root, file)
            with open(path, 'r') as f:
                content = f.read()
                # Naive regex for empty test functions
                # fn test_something() {}
                matches = re.finditer(r'fn\s+(test_\w+)\s*\(\s*\)\s*\{\s*\}', content)
                for match in matches:
                    print(f"{path}: {match.group(1)}")
