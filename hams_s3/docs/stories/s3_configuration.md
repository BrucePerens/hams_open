# Story: Configuring Amazon S3 Storage

## Context
Self-hosted instances often need to offload media/attachment storage to
Amazon S3 rather than the local filesystem, once the OCA `storage_backend`
family of modules is installed (see the module README for the setup
script). `hams_s3` adds the administrative UI for that, without hard-depending
on `storage_backend` itself so the module still installs cleanly before an
admin has run the setup script.

## The Solution
1. **Settings**: Administrators enable "Use Amazon S3 Storage" and enter the
   host, access key, secret key, bucket, and region from the "Cloud Storage"
   block Odoo's General Settings page gains once this module is installed
   (`COMM_hams_s3_config`). Saving writes (or updates) the matching
   `storage.backend` record via the module's own service account, rather
   than a hand-rolled `.sudo()` call.
2. **Rendering**: The settings block is injected into the standard
   `base_setup` settings form via an xpath extension whose compiled arch is
   proven directly (`get_view`) rather than through a browser tour, since a
   full "installed + configured" round-trip needs the OCA `storage_backend`
   addon actually present (`COMM_hams_s3_settings_render`).

## Impact
Administrators configure S3-backed attachment storage entirely from the
Odoo UI once the OCA dependency is installed, without ever needing direct
database or filesystem access to Odoo's storage configuration.

## Anchor reference

One entry per anchor named in the story, read from `models/res_config_settings.py`, `views/res_config_settings_views.xml` and `tests/test_res_config_settings.py`.

### `COMM_hams_s3_config` [@ANCHOR: COMM_hams_s3_config]

The S3 fields that `hams_s3` adds to the transient model `res.config.settings` (`models/res_config_settings.py`), with the two methods that load and save them.

| Field | Type | Meaning |
|---|---|---|
| `hams_s3_use_s3` | Boolean | "Use Amazon S3 Storage"; stored as the system parameter `hams_s3.use_s3_storage`. |
| `hams_s3_oca_installed` | Boolean, computed | True when the OCA model `storage.backend` is present; hides the detail fields otherwise. |
| `hams_s3_aws_host`, `hams_s3_aws_access_key_id`, `hams_s3_aws_bucket` | Char | Copied to the backend record. |
| `hams_s3_aws_secret_access_key` | Char | Secret key; shown masked as `********` when one is stored. |
| `hams_s3_aws_region` | Selection | One of eleven regions or `other`; empty saves as `us-east-1`. |

**`get_values()`** (`@api.private`): when `storage.backend` exists it reads the first `amazon_s3` backend as the `hams_s3.s3_manager_service_internal` service account and fills the form, replacing the stored secret with the mask so it is never sent to the browser. **`set_values()`** (`@public_rpc`): saves the parameter, registers a newly entered secret with the optional secret canary (a failure only logs), and when S3 is enabled and `storage.backend` exists creates or updates the single backend named "Amazon S3 Storage" as the service account. A masked or empty secret leaves the stored key unchanged. Nothing is written when `storage.backend` is absent. The service-account access to `storage.backend` is unverified against a real OCA install (to-do, noted in the code). Test: `test_set_and_get_values`.

### `COMM_hams_s3_settings_render` [@ANCHOR: COMM_hams_s3_settings_render]

The settings form extension `hams_s3.res_config_settings_view_form` (`views/res_config_settings_views.xml`), an `ir.ui.view` that inherits `base_setup.res_config_settings_view_form`. Its xpath `//block[@name='integration']` with `position="after"` inserts a block "Cloud Storage" (`hams_cloud_storage`) holding one setting, "S3 Storage": the checkbox `hams_s3_use_s3` and, shown only when it is ticked and `hams_s3_oca_installed` is true, the host, access key, secret key (a password widget), bucket and region fields. The view is for administrators on the Settings screen. Because the "installed and configured" state needs the OCA `storage_backend` addon, it is proven by a render test, not a browser tour: `test_settings_view_renders` compiles the combined arch, calls `get_view` and asserts `hams_cloud_storage` and `hams_s3_use_s3` are present. The view cites this anchor as its audit-ignore reason. Failure means the base settings view changed so the xpath no longer matches.
