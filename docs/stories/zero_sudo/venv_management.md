# Story: Python VENV Management `[@ANCHOR: story_venv_management]`

This story describes how administrators can trigger an update of the system's Python dependencies.

## Background
Odoo modules might have external Python dependencies listed in a `requirements.txt` file.

## The Process
1. **Admin Trigger**: A user with "System Administrator" privileges triggers an environment update.
2. **Access Verification**: The `_update_python_venv` function `[@ANCHOR: update_python_venv]` verifies the user's group.
3. **Path Resolution**: It locates the `requirements.txt` file relative to the module's directory.
4. **Execution**: It runs `pip install` using the current Python executable to ensure all listed packages are installed in the environment.

## Safety
This function is restricted to global administrators and uses `subprocess.run` with `shell=False` to prevent shell injection.
