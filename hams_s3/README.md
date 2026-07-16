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
