import os
import re

def unstack_file(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        lines = f.readlines()
    
    out = []
    prev_has_anchor = False
    modified = False
    
    for line in lines:
        has_anchor = "[@ANCHOR:" in line
        if has_anchor and prev_has_anchor:
            out.append("\n")
            modified = True
        out.append(line)
        prev_has_anchor = has_anchor
        
    if modified:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.writelines(out)
        print(f"Unstacked {filepath}")

for root, dirs, files in os.walk('/home/bruce/workspace/hams_open'):
    if '.git' in root or 'node_modules' in root or '__pycache__' in root:
        continue
    for file in files:
        if file.endswith('.md') or file.endswith('.py') or file.endswith('.html'):
            unstack_file(os.path.join(root, file))
