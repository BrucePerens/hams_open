# Data structures

### `ResConfigSettings` [@ANCHOR: hams_s3:hams_s3_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model 'res.config.settings' with the fields `hams_s3_use_s3`, `hams_s3_oca_installed`, `hams_s3_aws_host`, `hams_s3_aws_access_key_id`, `hams_s3_aws_secret_access_key`, `hams_s3_aws_bucket`, `hams_s3_aws_region` and the methods `_get_s3_service_env`, `_compute_hams_s3_oca_installed`, `get_values`, `set_values`, `_register_s3_secret_with_canary`. Defined in `hams_s3/models/res_config_settings.py:15` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `hams_s3_use_s3` | `Boolean` | Use Amazon S3 Storage: Enable and configure the Amazon S3 storage backend. |
| `hams_s3_oca_installed` | `Boolean` | OCA Storage Installed (compute=`"_compute_hams_s3_oca_installed"`) |
| `hams_s3_aws_host` | `Char` | AWS Host |
| `hams_s3_aws_access_key_id` | `Char` | AWS Access Key |
| `hams_s3_aws_secret_access_key` | `Char` | AWS Secret Key |
| `hams_s3_aws_bucket` | `Char` | AWS Bucket |
| `hams_s3_aws_region` | `Selection` | AWS Region (values `us-east-1`, `us-east-2`, `us-west-1`, `us-west-2`, `eu-west-1`, `eu-central-1`, `ap-southeast-1`, `ap-southeast-2`, `ap-northeast-1`, `sa-east-1`, `other`) |

**Class attributes.** `_inherit = 'res.config.settings'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_s3_service_env`, `_compute_hams_s3_oca_installed`, `get_values`, `set_values`, `_register_s3_secret_with_canary`.

**See also:** test `test_set_and_get_values (tests/test_res_config_settings.py)`.
