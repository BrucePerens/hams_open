# Problems in Jules VM Environment

## 1. Provisioning Issues
- Initial provisioning command `IN_JULES_VM=1 python3 tools/test.py --provision-jules` timed out after 400 seconds. This is likely due to the large number of APT packages being installed and the environment's resource constraints.
- Subsequent attempt to run provisioning in the background succeeded.

## 2. Testing Issues
- Standard tests for `manual_library` failed with `PermissionError: [Errno 13] Permission denied: '/home/jules/.local'`.
- This occurred during database initialization while Odoo (running as `odoo` user) tried to create a directory in the `jules` user's home directory.
- Specifically, `os.makedirs('/home/jules/.local', exist_ok=True)` failed because the `odoo` user does not have permission to write to `/home/jules`.
- Error Traceback:
  ```
  File "/usr/lib/python3/dist-packages/odoo/addons/base/models/ir_attachment.py", line 139, in _get_path
    os.makedirs(dirname, exist_ok=True)
  File "<frozen os>", line 215, in makedirs
  PermissionError: [Errno 13] Permission denied: '/home/jules/.local'
  ```
- This prevented the database `zero_sudo` from being initialized, thus tests could not run.
