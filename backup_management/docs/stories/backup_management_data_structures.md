# Data structures

### `BackupBoard` [@ANCHOR: backup_management:backup_management_board_backup_board]

**Purpose and location.** The class has no docstring; from its definition it provides `backup_board`. Defined in `backup_management/controllers/board.py:8` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `backup_board`.

**See also:** test `test_board_security_and_render (tests/test_backup_board_controller.py)`.

### `_NoRedirect` [@ANCHOR: backup_management:backup_management_main_no_redirect]

**Purpose and location.** Refuses every redirect. urllib copies all request headers, the credential included, to wherever a 3xx answer points; a refused redirect surfaces as the HTTPError of the 3xx status. Defined in `backup_management/daemon/main.py:25` as a class; bases: `urllib.request.HTTPRedirectHandler`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `redirect_request`.

**See also:** test `test_a_redirect_from_odoo_is_refused_and_the_key_never_reaches_the_second_host (daemon/test_main.py)`.

### `OdooAPIError` [@ANCHOR: backup_management:backup_management_main_odoo_apierror]

**Purpose and location.** Custom exception for Odoo JSON-2 API failures. Defined in `backup_management/daemon/main.py:101` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_a_401_with_an_unchanged_or_unreadable_key_file_is_still_an_error_without_a_retry_loop (daemon/test_main.py)`.

### `BackupConfig` [@ANCHOR: backup_management:backup_management_backup_config_backup_config]

**Purpose and location.** Model description: Backup Configuration. Defined in `backup_management/models/backup_config.py:18` as an Odoo model; bases: `models.Model`. (model `backup.config`; inherits ["mail.thread"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; default=`lambda self: self.env.company`) |
| `engine` | `Selection` | no label or help in the source (values `kopia`, `pgbackrest`; required=`True`) |
| `target_path` | `Char` | Target / Stanza: Repository path for Kopia, or Stanza name for pgBackRest. (required=`True`) |
| `minimum_size_mb` | `Integer` | Minimum Size (MB): Triggers a Pager Duty alert if a new snapshot is smaller than this. (default=`0`) |
| `kopia_password_crypt` | `Char` | Encrypted Kopia Password |
| `kopia_password` | `Char` | Kopia Password (compute=`"_compute_kopia_password"`) |
| `storage_type` | `Selection` | Storage Type (values `local`, `s3`, `b2`; required=`True`; default=`"local"`) |
| `bucket_name` | `Char` | Bucket Name |
| `endpoint_url` | `Char` | Endpoint URL |
| `region` | `Char` | Region: S3 region for the pgBackRest repository (e.g. 'eu-central-1'). Optional -- AWS S3 and Backblaze B2 both tolerate the daemon's own 'us-east-1' placeholder default, but a strict S3-compatible provider that enforces SigV4 region/endpoint agreement needs the real region set here. |
| `pgbackrest_repo` | `Integer` | pgBackRest Repository Number: 0 (default): the daemon describes repository 1 from the storage fields above. A number N from 1 up: use repositoryN as defined in /etc/pgbackrest/pgbackrest.conf on the database host (backups and snapshot listings run with --repo=N). Use this for an encrypted repository, whose passphrase belongs only in that file and is never stored in Odoo. Required by pgBackRest for backups when more than one repository is configured. (default=`0`) |
| `access_key` | `Char` | Access Key |
| `secret_key_crypt` | `Char` | Encrypted Secret Key |
| `secret_key` | `Char` | Secret Key (compute=`"_compute_secret_key"`) |
| `keep_daily` | `Integer` | Keep Daily (default=`7`) |
| `keep_weekly` | `Integer` | Keep Weekly (default=`4`) |
| `keep_monthly` | `Integer` | Keep Monthly (default=`6`) |
| `exclude_patterns` | `Text` | Exclude Patterns (.kopiaignore): One pattern per line. |
| `restore_drill_script` | `Char` | Automated Restore Drill Script: Absolute path to a shell script that performs a test restore and data validation. |
| `last_drill_time` | `Datetime` | Last Successful Drill (readonly=`True`) |
| `snapshot_ids` | `One2many` | Snapshots (comodel `backup.snapshot`) |

**Class attributes.** `_name = "backup.config"`; `_description = "Backup Configuration"`; `_inherit = ["mail.thread"]`; `_SECRET_FIELD_GROUPS = "backup_management.group_backup_admin,"
        "backup_management.group_backup_service_account"`; `_name_uniq_msg = """The backup configuration name must be unique!"""`; `_name_uniq = models.Constraint("UNIQUE(name)", _name_uniq_msg)`; `_target_uniq_msg = """The target path must be unique per engine!"""`; `_target_uniq = models.Constraint("UNIQUE(engine, target_path)", _target_uniq_msg)`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_fernet`, `_crypt_field`, `_compute_encrypted_field`, `_inverse_encrypted_field`, `_compute_kopia_password`, `_inverse_kopia_password`, `_compute_secret_key`, `_inverse_secret_key`, `_check_security_paths`, `_get_executable`, `_publish_to_worker`, `action_trigger_backup`, `action_apply_policies`, `action_test_connection`, `action_view_latest_job`, `action_sync_snapshots`, `_execute_restore_drill`, `report_backup_failure`, `get_board_data`, `action_process_snapshot_data`, `_process_snapshot_data`, `_backup_job_problems`, `cron_sync_all_backups`, `cron_trigger_scheduled_backups`.

**See also:** test `test_01_builds_real_repo1_flags_no_secrets (daemon/test_main.py)`.

### `BackupJob` [@ANCHOR: backup_management:backup_management_backup_job_backup_job]

**Purpose and location.** Model description: Asynchronous Backup Job. Defined in `backup_management/models/backup_job.py:12` as an Odoo model; bases: `models.Model`. (model `backup.job`; default order "create_date desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `config_id` | `Many2one` | Configuration (comodel `backup.config`; required=`True`; ondelete=`"cascade"`) |
| `website_id` | `Many2one` | Website (comodel `website`; related=`"config_id.website_id"`; store=`True`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; related=`"config_id.company_id"`; store=`True`) |
| `job_type` | `Selection` | Job Type (values `kopia`, `pgbackrest`; required=`True`) |
| `state` | `Selection` | State (values `pending`, `processing`, `done`, `failed`; required=`True`; default=`"pending"`) |
| `output_log` | `Text` | Live Output Log |

**Class attributes.** `_name = "backup.job"`; `_description = "Asynchronous Backup Job"`; `_order = "create_date desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `append_log`, `_mark_dispatch_failed`, `_auto_refresh_status`, `_fail_stale_pending_jobs`, `action_refresh_status`.

**See also:** test `test_01_kopia_s3_job_connects_before_snapshot_and_hides_secrets (daemon/test_main.py)`.

### `BackupSnapshot` [@ANCHOR: backup_management:backup_management_backup_snapshot_backup_snapshot]

**Purpose and location.** Model description: Backup Snapshot. Defined in `backup_management/models/backup_snapshot.py:7` as an Odoo model; bases: `models.Model`. (model `backup.snapshot`; default order "start_time desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `config_id` | `Many2one` | Configuration (comodel `backup.config`; required=`True`; ondelete=`"cascade"`) |
| `website_id` | `Many2one` | Website (comodel `website`; related=`"config_id.website_id"`; store=`True`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; related=`"config_id.company_id"`; store=`True`) |
| `snapshot_id` | `Char` | Snapshot ID / Label (required=`True`) |
| `start_time` | `Datetime` | Start Time |
| `size_bytes` | `Float` | Size (Bytes) |
| `status` | `Char` | Status |
| `restore_command` | `Char` | Restore Command (compute=`"_compute_restore_command"`) |

**Class attributes.** `_name = "backup.snapshot"`; `_description = "Backup Snapshot"`; `_order = "start_time desc"`; `_snapshot_uniq_msg = """Snapshot IDs must be unique per configuration!"""`; `_snapshot_uniq = models.Constraint("UNIQUE(config_id, snapshot_id)", _snapshot_uniq_msg)`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_restore_command`.

**See also:** test `test_01_kopia_s3_job_connects_before_snapshot_and_hides_secrets (daemon/test_main.py)`.

### `BackupRestoreWizard` [@ANCHOR: backup_management:backup_management_restore_wizard_backup_restore_wizard]

**Purpose and location.** Model description: Backup Restore Wizard. Defined in `backup_management/models/restore_wizard.py:11` as an Odoo model; bases: `models.TransientModel`. (model `backup.restore.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `snapshot_id` | `Many2one` | Snapshot (comodel `backup.snapshot`; required=`True`; readonly=`True`) |
| `restore_target_path` | `Char` | Restore Directory / Stanza Target: Path where the backup should be restored, or stanza to target. (required=`True`) |
| `engine` | `Selection` | Backup Engine (related=`"snapshot_id.config_id.engine"`; readonly=`True`) |

**Class attributes.** `_name = "backup.restore.wizard"`; `_description = "Backup Restore Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_restore`.

**See also:** test `test_01_every_secret_key_is_masked_and_the_rest_is_kept (daemon/test_main.py)`.

### `BackupLatestSnapshotView` [@ANCHOR: backup_management:backup_management_sql_views_backup_latest_snapshot_view]

**Purpose and location.** Model description: Latest Backup Snapshot View. Defined in `backup_management/models/sql_views.py:7` as an Odoo model; bases: `models.Model`. (model `backup.latest.snapshot.view`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `config_id` | `Many2one` | no label or help in the source (comodel `backup.config`; readonly=`True`) |
| `website_id` | `Many2one` | no label or help in the source (comodel `website`; readonly=`True`) |
| `company_id` | `Many2one` | no label or help in the source (comodel `res.company`; readonly=`True`) |
| `snapshot_id` | `Char` | no label or help in the source (readonly=`True`) |
| `start_time` | `Datetime` | no label or help in the source (readonly=`True`) |
| `size_bytes` | `Float` | no label or help in the source (readonly=`True`) |
| `status` | `Char` | no label or help in the source (readonly=`True`) |

**Class attributes.** `_name = "backup.latest.snapshot.view"`; `_description = "Latest Backup Snapshot View"`; `_auto = False`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_01_kopia_s3_job_connects_before_snapshot_and_hides_secrets (daemon/test_main.py)`.
