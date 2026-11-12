# Data structures

### `DaemonKeyRegistry` [@ANCHOR: daemon_key_manager:daemon_key_manager_key_registry_daemon_key_registry]

**Purpose and location.** Daemon API Key Registry. This model is multi-tenant (company-aware) because service accounts and their associated API keys are bound to a specific company context. Daemons operating for different companies must have separate registry entries to maintain strict security isolation. Defined in `daemon_key_manager/models/key_registry.py:56` as an Odoo model; bases: `models.Model`. (model `daemon.key.registry`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Daemon Name (required=`True`) |
| `user_id` | `Many2one` | Service Account (comodel `res.users`; required=`True`) |
| `env_file_path` | `Char` | Environment File Path:          Absolute path to the protected output directory for this daemon's .env file.         Must start with /opt/hams/etc/keys/.          (required=`True`) |
| `company_id` | `Many2one` | Company: The company that owns this daemon registry. Service accounts are company-specific. (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `last_rotated` | `Datetime` | Last Rotated (readonly=`True`) |
| `os_group` | `Char` | OS Group:          Operating-system group of the account the daemon runs as (hamsd_<family>). When set,         the key file is written 0640 with that group, so that account and no other daemon         can read it. When empty the file is 0600 and readable only by the Odoo user, as         before. The Odoo user must be a member of the group (provisioning adds it) and         the server must have been restarted since; otherwise the write fails and the old         key file is left untouched.          |
| `remote_self_rotation` | `Boolean` | Remote Self-Rotation:          Set for a daemon that runs on a different machine than Odoo. Odoo         cannot write that machine's key file, so the Odoo-side rotation         paths (the daily cron, Force Provision All, Rotate Key) skip this         registry, and the daemon rotates its own key through         rotate_own_key() over JSON-2 instead.          (default=`False`) |
| `key_lifetime_days` | `Integer` | Key Lifetime (days):          0 uses the module's 90-day lifetime. A remote self-rotating daemon whose operator does not         want it to depend on a rotation window can ask for longer, from 90 up to 400 days (Bruce's         decision for the pi500-1 FCC ULS key: at least one year). It applies to keys this registry         mints from now on, and "Apply Key Lifetime" extends the key the daemon holds today. The         daemon is due to rotate 31 days before its key expires, the same margin as the 59-day         rotation of a 90-day key. Only a remote self-rotating row may set it: an Odoo-rotated         key keeps the module's schedule.          (default=`0`) |
| `pending_key_id` | `Integer` | no label or help in the source (readonly=`True`; copy=`False`) |

**Class attributes.** `_name = "daemon.key.registry"`; `_description = "Daemon API Key Registry"`; `_err_uniq = "The daemon name must be unique per company!"`; `_name_company_uniq = models.Constraint(
        "unique(name, company_id)", _err_uniq
    )`; `_err_name = "The daemon name cannot be empty."`; `_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", _err_name
    )`; `_err_path = "The environment file path cannot be empty."`; `_chk_path = "CHECK(LENGTH(TRIM(env_file_path)) > 0)"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_user_is_service_account`, `_check_os_group`, `_check_env_file_path`, `register_daemon`, `unregister_daemon`, `_check_key_lifetime_days`, `_lifetime_days`, `_rotation_age_days`, `action_apply_key_lifetime`, `_ensure_usage_group`, `_grant_usage_group`, `action_force_provision_all`, `action_rotate_key`, `rotate_own_key`, `_own_key_row_id`, `_issue_remote_rotation`, `_confirm_remote_rotation`, `_rotate_key_and_write_file`, `_mint_key`, `_assert_account_may_hold_key`, `_resolve_os_group`, `_write_secure_env_file`, `_cron_rotate_all_keys`.

**See also:** test `test_security_constraints (tests/test_key_registry.py)`.

### `ResCompany` [@ANCHOR: daemon_key_manager:daemon_key_manager_res_company_res_company]

**Purpose and location.** Extends the Odoo model 'res.company' with the fields `daemon_registry_ids`. Defined in `daemon_key_manager/models/res_company.py:8` as an Odoo model; bases: `models.Model`. (extends `res.company`.)

| Field | Type | Meaning |
|---|---|---|
| `daemon_registry_ids` | `One2many` | Daemon Registries (comodel `daemon.key.registry`) |

**Class attributes.** `_inherit = 'res.company'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_manager_group_has_intentional_cross_company_reach (tests/test_multi_company.py)`.

### `ResUsers` [@ANCHOR: daemon_key_manager:daemon_key_manager_res_users_res_users]

**Purpose and location.** Extends the Odoo model 'res.users' with the fields `daemon_registry_ids`. Defined in `daemon_key_manager/models/res_users.py:8` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

| Field | Type | Meaning |
|---|---|---|
| `daemon_registry_ids` | `One2many` | Daemon Registries (comodel `daemon.key.registry`) |

**Class attributes.** `_inherit = 'res.users'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_rotating_again_revokes_the_previous_key (tests/test_key_registry.py)`.
