# Jules Environment Issues for user_websites_seo

This document records issues encountered while provisioning the environment and running tests for the `user_websites_seo` module in the Jules VM.

## Provisioning Issues
- No major issues during provisioning, although it took a significant amount of time due to the large number of APT packages being installed.

## Test Issues
- Standard tests failed for `user_websites_seo` with a `PermissionError`.
- Error: `PermissionError: [Errno 13] Permission denied: '/home/jules/.local'`
- Context: Odoo (running as `odoo` user) attempted to create a directory in `/home/jules/.local` while loading the `base` module (specifically `res_lang_data.xml`). This appears to be due to the `HOME` environment variable being preserved as `/home/jules` when running via `sudo -E -u odoo`.
- Log snippet:
```
  File "/usr/lib/python3/dist-packages/odoo/addons/base/models/ir_attachment.py", line 139, in _get_path
    os.makedirs(dirname, exist_ok=True)
  File "<frozen os>", line 215, in makedirs
  File "<frozen os>", line 215, in makedirs
  File "<frozen os>", line 215, in makedirs
  [Previous line repeated 2 more times]
  File "<frozen os>", line 225, in makedirs
PermissionError: [Errno 13] Permission denied: '/home/jules/.local'
```
