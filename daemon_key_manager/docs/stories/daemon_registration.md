# Story: Registering a New External Daemon

As a **Developer** of an external Odoo integration (like a monitoring service or a backup manager),
I want to **register my daemon** with the Odoo instance,
so that it receives a secure, auto-rotating API key without me having to manage credentials manually.

## Scenario: Initial Module Installation

1.  The developer creates a new Odoo module for their daemon.
2.  In the module's `post_init_hook` or a configuration wizard, they call the `register_daemon` API.

3.  The `daemon_key_manager` verifies the service account and the requested file path.
4.  A new entry is created in the `daemon.key.registry`.
5.  An initial API key is generated and securely written to the specified `.env` file on disk.
6.  The daemon can now boot and read its credentials from `/opt/hams/etc/keys/my_daemon.env`.

## Key Logic
- The registration is idempotent; calling it again updates the existing registration.

- Security constraints ensure that only service accounts can be used and files are written to allowed paths.

- Automated privilege assignment ensures the daemon has the correct permissions for long-lived keys.

## Scenario: A daemon that runs under its own OS account

1.  The daemon's unit runs as a dedicated account (`hamsd_<family>`), not as the Odoo user.
2.  A Daemon Key Manager registers it with that group (`register_daemon(..., os_group="hamsd_<family>")`) [@ANCHOR: COMM_register_daemon_os_group_manager_only].
3.  The key file is written `0640` with that group into the family's own key directory (`/opt/hams/etc/keys/<family>`, owned by Odoo, that group, `0750`), so only that account reads it [@ANCHOR: COMM_write_secure_env_file_group] [@ANCHOR: COMM_write_secure_env_file_group_directory].
4.  Every other daemon, which is in no such group, gets "permission denied" opening it.
5.  The group is checked before any file is written: a name that is not `hamsd_<family>`, or a group this host does not have, is refused with an error that says so (and, for a missing group, to run the provisioning that creates the daemon account). A key file is never written under another group, or under the Odoo user's own, because provisioning has not run yet, and the previous key file is left untouched [@ANCHOR: COMM_resolve_os_group].
6.  Where the file may live follows from the group alone: the directory for `hamsd_<family>` is `/opt/hams/etc/keys/<family>`, and no other. Two families therefore never share a directory, and a row cannot point a family's key at the shared key root or at another family's directory [@ANCHOR: COMM_key_directory_for_group].

## Entries

### `DaemonKeyRegistry.register_daemon(self, daemon_name, user_xml_id, env_file_path, os_group=None) -> True` [@ANCHOR: COMM_register_daemon_api]

The API other modules call (usually from `post_init_hook`) to get an API key for a daemon. Private to RPC (`@api.private`). Arguments: `daemon_name` (the registry name, unique per company), `user_xml_id` (the service account, as an XML id with a dot or as a login), `env_file_path` (must be under `/opt/hams/etc/keys/`) and `os_group` (optional `hamsd_<family>`; None leaves it alone, an empty string clears it). A caller who is not a Daemon Key Manager must be an administrator, the superuser, or the service account registering itself; setting `os_group` needs a manager. Runs as the manager service account, creates or updates the registry row, flushes (so constraints run before any file I/O) and rotates and writes the key synchronously. Returns True. Errors: `AccessError`, `UserError` (unknown login). Tests: `tests/test_key_registry.py` `test_register_daemon_api`.

### `register_daemon`: calling it again updates the row [@ANCHOR: COMM_register_daemon_idempotency]

Registration is idempotent. When a registry with the same name already exists for the account's company, the call writes the new `user_id`, `env_file_path`, `company_id` (and `os_group` when given) onto it instead of creating a second row, then rotates and rewrites the key as for a new one. A module can therefore call it on every install or upgrade without duplicates, and a moved key file is picked up. The unique constraints on name (per company) and on `env_file_path` still apply, so two daemons cannot be pointed at one file. Tests: `test_register_daemon_api`.

### `DaemonKeyRegistry._check_user_is_service_account(self)` [@ANCHOR: COMM_security_constraints_user]

An `api.constrains("user_id")` check: the registry's account must have `is_service_account` set, otherwise a `UserError` ("The selected user must be a service account.") aborts the create or write. A key for a human user, which could carry that person's wider rights, can therefore never be minted. No arguments, no return value, no side effects beyond the error. It runs when the registry row is flushed, so it fires before `register_daemon` writes any file. Tests: `tests/test_key_registry.py` `test_security_constraints`.

### `DaemonKeyRegistry._check_env_file_path(self)` [@ANCHOR: COMM_security_constraints_path]

An `api.constrains("env_file_path")` check. A blank path is skipped. A path with a `..` component is refused as directory traversal, then the normalised path is resolved with `os.path.realpath` (so a symlink cannot point out) and must start with `/opt/hams/etc/keys/`; otherwise a `UserError` ("Security Alert...") is raised naming the resolved path. It touches the filesystem only to resolve links; it creates nothing. The writer repeats the same check just before opening a file, so a row that predates the rule is still caught. Tests: `test_security_constraints`.

### `DaemonKeyRegistry._grant_usage_group(self, user, usage_group)` [@ANCHOR: COMM_privilege_escalation_bypass]

Adds a service account to a key-usage group (`group_daemon_key_usage`, 90-day keys, or the `_long` group for lifetimes above 90 days) with a direct `INSERT ... ON CONFLICT DO NOTHING` into `res_groups_users_rel`, then invalidates the user's cache and the registry cache. Arguments: the `user` and the `usage_group` record; nothing happens when the group is missing or the user already has it. Raw SQL is used because granting a group through the ORM needs the Settings administrator right, which this service account must not hold (zero-sudo). It runs from `_ensure_usage_group` on every rotation, so a module upgrade that replaced the account's group set is healed. Tests: `test_action_rotate_key_self_heals_a_service_account_missing_the_usage_group`.

### `register_daemon`: find or create the registry row [@ANCHOR: COMM_register_daemon_logic]

The middle of `register_daemon`. After the caller has been authorised it looks for an existing registry with the same `name` in the service account's company (multi-company aware), builds the values `user_id`, `env_file_path`, `company_id` and optionally `os_group`, and creates the row when none exists. Creation and update both go through the model constraints (service account, path prefix, OS group), which run at the following `flush_all`, before any file is touched, so a refused registration cannot leave a half-written key file. The key itself is made afterwards by `_rotate_key_and_write_file`. Tests: `test_register_daemon_api`.

### `DaemonKeyRegistry._write_secure_env_file(self, path, login, key, group=None)` [@ANCHOR: COMM_write_secure_env_file_logic]

Writes a daemon's credentials to its key file. Arguments: `path` (resolved with `realpath`; must start with `/opt/hams/etc/keys/` or a `UserError` is raised, whatever the registry constraint said), `login`, `key`, and an optional `hamsd_<family>` `group`. Directories are created at `0700` (the key root is kept at `0710`; a family directory is handed to its group at `0750`). The content (`ODOO_RPC_LOGIN`, `ODOO_RPC_KEY`) goes to a fresh temporary file made `0600` by this process and is renamed over the target, so a pre-existing file with loose permissions can never receive a live key and a failed write leaves the old key intact; with a group the file becomes `0640` with that group. Returns nothing; permission and OS errors are logged and re-raised. Tests: `test_register_daemon_api`.
