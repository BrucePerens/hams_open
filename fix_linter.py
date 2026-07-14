import os
import re

def fix_inits(root_dir):
    for dirpath, _, filenames in os.walk(root_dir):
        if "__init__.py" in filenames:
            init_path = os.path.join(dirpath, "__init__.py")
            with open(init_path, "r", encoding="utf-8") as f:
                content = f.read()
            # Replace `from . import models` with `from . import models  # noqa: F401`
            lines = content.splitlines()
            new_lines = []
            for line in lines:
                if line.startswith("from . ") and "# noqa: F401" not in line:
                    line = f"{line}  # noqa: F401"
                new_lines.append(line)
            
            # remove W391 (blank line at end)
            while new_lines and not new_lines[-1].strip():
                new_lines.pop()
                
            with open(init_path, "w", encoding="utf-8") as f:
                f.write("\n".join(new_lines) + "\n")

def fix_all(root_dir):
    for dirpath, _, filenames in os.walk(root_dir):
        for filename in filenames:
            if filename.endswith(".py"):
                path = os.path.join(dirpath, filename)
                with open(path, "r", encoding="utf-8") as f:
                    lines = f.readlines()
                new_lines = []
                for line in lines:
                    # Fix E501 in copyright
                    if "Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License" in line:
                        new_lines.append("# Copyright © Bruce Perens K6BP.\n")
                        new_lines.append("# Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).\n")
                    else:
                        new_lines.append(line)
                # Remove W391 trailing newlines
                while new_lines and new_lines[-1] in ('\n', '\r\n') and (len(new_lines) > 1 and new_lines[-2] in ('\n', '\r\n')):
                    new_lines.pop()
                
                with open(path, "w", encoding="utf-8") as f:
                    f.writelines(new_lines)

fix_all("hams_open/compliance")
fix_inits("hams_open/compliance")
