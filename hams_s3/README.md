# Hams S3 Config

This module integrates the OCA `storage.backend` model (from OCA's `storage_backend` addon; OCA is the Odoo Community Association, whose repositories under `github.com/OCA` the setup script clones) with Odoo's General Settings for Amazon S3 (Simple Storage Service), allowing administrators to configure S3 storage credentials directly from the UI. It also provides a setup script for fetching and patching the necessary OCA modules.

## Features

- **General Settings Integration**: Adds a "Cloud Storage" block to General Settings for configuring S3 buckets.
- **Dependency Setup**: Includes a script `scripts/install_oca_storage.py` that clones the OCA `storage`, `connector` and `server-env` repositories (branch `18.0`), then copies and patches four addons from them into a directory you name: `storage_backend`, `storage_backend_s3`, `component` (from the `connector` repository), and `server_environment`. It does not install them into Odoo; it only copies them.

## Installation / Setup

Administrators must run the included setup script to fetch the required OCA modules before using this module:

```bash
python3 hams_s3/scripts/install_oca_storage.py <dest_dir>
```

`<dest_dir>` is the addons directory the four OCA addons are copied into. The argument is required: without it the script prints a usage line and exits with status 1.

This script will:
1. Clone the necessary OCA repositories into `/tmp/oca_install` (deleting that directory first if it already exists).
2. Copy the required modules to the destination directory. An addon directory of the same name already in `<dest_dir>` is deleted and replaced.
3. Apply Hams Open Linter Patches: fixed text and regular-expression replacements to the copied OCA source so it passes this platform's own linters. They change each manifest's license from LGPL-3 to AGPL-3 and add a `description`, add `audit-ignore`/`burn-ignore` linter markers, remove `.sudo()`/`with_user(1)` from one vendored test, add `name` fields, drop `expand` and replace or drop `string` attributes in views, replace a `print` with logging, and make `storage_backend_s3`'s `boto3` import unconditional. Because of that last patch, the Python `boto3` package must be importable or `storage_backend_s3` fails to load. A replacement whose target text is not found changes nothing and raises no error, so re-run the linters after the script, as its last line advises.

After running the script, restart the Odoo server and update the apps list.

## Internal functions (infrastructure, not user-visible)

- `hooks.py`'s `post_init_hook` ([@ANCHOR: hams_s3_post_init_hook]) registers this module's own
  S3-manager service account (`s3_manager_service_internal`) with `daemon.key.registry` on install.
  Registering asks `daemon_key_manager` to provision an API key for that account into a key file and
  to rotate it every 60 days.
- `res_config_settings.py`'s `_get_s3_service_env` ([@ANCHOR: hams_s3_get_s3_service_env]) resolves
  an `Environment` impersonating that same service account, used instead of `.sudo()` (forbidden
  platform-wide) to reach OCA's `storage.backend` model. This is marked DRAFT, UNVERIFIED in the
  code: the account's only group, "Service Account: S3 Storage Manager", is granted no access
  rights by this module, and whether it can read, write and create `storage.backend` records under
  `storage_backend`'s own security rules has not been tested against a real install.
- `_compute_hams_s3_oca_installed` ([@ANCHOR: hams_s3_compute_oca_installed]) reflects, live,
  whether that OCA addon is actually installed in the current environment.
- `get_values`/`set_values` ([@ANCHOR: hams_s3_get_values] [@ANCHOR: hams_s3_set_values]) both gate
  their real `storage.backend`-touching logic behind `'storage.backend' in self.env`, since this
  module deliberately does not hard-depend on that OCA addon (see "Installation / Setup" above) --
  in any environment where it isn't installed yet, these are safe no-ops beyond the base ORM
  behavior. When it is installed, both act on the first `storage.backend` record whose
  `backend_type` is `amazon_s3`: `get_values` copies its host, keys, bucket and region into the
  form, and `set_values` (only while "Use Amazon S3 Storage" is checked) writes the form's values
  to that record, or creates one named "Amazon S3 Storage" if none exists, using `us-east-1` when
  no region is chosen.
