import sys
import re

f = open('target/llvm-cov/html/coverage/app/crates/tsuzulint_core/src/rule_manifest.rs.html').read()
rows = re.findall(r'<tr>(.*?)</tr>', f, re.DOTALL)
for r in rows:
    if 'class=\'uncovered-line\'' in r or 'class="uncovered-line"' in r or 'class="region red"' in r or 'class=\'region red\'' in r:
        num_m = re.search(r'<a name=.L(\d+).', r)
        num = num_m.group(1) if num_m else "?"
        code_m = re.search(r'<td class=.code.><pre>(.*?)</pre></td>', r, re.DOTALL)
        code = code_m.group(1) if code_m else ""
        print(f"Line {num}: {code}")
