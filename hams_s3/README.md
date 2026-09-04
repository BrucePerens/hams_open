# Hams S3 Config

This module integrates the OCA `storage.backend` with Odoo's General Settings for Amazon S3, allowing administrators to configure S3 storage credentials directly from the UI. It also provides a setup script for fetching and patching the necessary OCA modules.

## Features

- **General Settings Integration**: Adds a "Cloud Storage" block to General Settings for configuring S3 buckets.
- **Dependency Setup**: Includes a script `scripts/install_oca_storage.py` that clones, patches, and installs `storage.backend`, `storage_backend_s3`, `connector`, and `server_environment` from the OCA.

## Installation / Setup

Administrators must run the included setup script to fetch the required OCA modules before using this module:

```bash
python3 hams_s3/scripts/install_oca_storage.py
```

This script will:
1. Clone the necessary OCA repositories into `/tmp/oca_install`.
2. Copy the required modules to the destination directory.
3. Apply Hams Open Linter Patches to ensure compatibility with internal linting standards (e.g., AGPL-3 licensing, python syntax fixes).

After running the script, restart the Odoo server and update the apps list.

## Internal functions (infrastructure, not user-visible)

- `hooks.py`'s `post_init_hook` ([@ANCHOR: hams_s3_post_init_hook]) registers this module's own
  S3-manager service account (`s3_manager_service_internal`) with `daemon.key.registry` on install.
- `res_config_settings.py`'s `_get_s3_service_env` ([@ANCHOR: hams_s3_get_s3_service_env]) resolves
  an `Environment` impersonating that same service account, used instead of `.sudo()` (forbidden
  platform-wide) to reach OCA's `storage.backend` model.
- `_compute_hams_s3_oca_installed` ([@ANCHOR: hams_s3_compute_oca_installed]) reflects, live,
  whether that OCA addon is actually installed in the current environment.
- `get_values`/`set_values` ([@ANCHOR: hams_s3_get_values] [@ANCHOR: hams_s3_set_values]) both gate
  their real `storage.backend`-touching logic behind `'storage.backend' in self.env`, since this
  module deliberately does not hard-depend on that OCA addon (see "Installation / Setup" above) --
  in any environment where it isn't installed yet, these are safe no-ops beyond the base ORM
  behavior.
