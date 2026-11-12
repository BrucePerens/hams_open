# Journey: Lifecycle of a Daemon API Key

This journey describes the full lifecycle of a daemon's credentials, from registration to continuous rotation.

1.  **Birth: Registration**
    - A module calls `register_daemon()`.

    - `daemon_key_manager` creates a record and generates the first key.

    - The key is written to a secure `.env` file.

2.  **Usage: Consumption**
    - The external daemon boots and reads `ODOO_RPC_KEY` from the file.
    - It uses this key for JSON-RPC authentication to Odoo.
    - Odoo validates the key against the service account.

3.  **Renewal: Periodic Rotation**
    - After 60 days, the cron job triggers.

    - `daemon_key_manager` revokes the old key and generates a new one.
    - The `.env` file is updated with the new key.

4.  **Transition: Handover**
    - The daemon makes a call with the old key and receives an `AccessError`.
    - The daemon catches the error, re-reads the `.env` file.
    - The daemon retries with the new key and continues its work.

5.  **End of Life: Uninstallation**
    - If the daemon's module is uninstalled, its registry entry may be removed.
    - (Optional) A manual cleanup of the `.env` file can be performed by an admin.

## Entries

### `DaemonKeyRegistry.register_daemon(self, daemon_name, user_xml_id, env_file_path, os_group=None) -> True` [@ANCHOR: COMM_register_daemon_api]

The API other modules call (usually from `post_init_hook`) to get an API key for a daemon. Private to RPC (`@api.private`). Arguments: `daemon_name` (the registry name, unique per company), `user_xml_id` (the service account, as an XML id with a dot or as a login), `env_file_path` (must be under `/opt/hams/etc/keys/`) and `os_group` (optional `hamsd_<family>`; None leaves it alone, an empty string clears it). A caller who is not a Daemon Key Manager must be an administrator, the superuser, or the service account registering itself; setting `os_group` needs a manager. Runs as the manager service account, creates or updates the registry row, flushes (so constraints run before any file I/O) and rotates and writes the key synchronously. Returns True. Errors: `AccessError`, `UserError` (unknown login). Tests: `tests/test_key_registry.py` `test_register_daemon_api`.

### `register_daemon`: find or create the registry row [@ANCHOR: COMM_register_daemon_logic]

The middle of `register_daemon`. After the caller has been authorised it looks for an existing registry with the same `name` in the service account's company (multi-company aware), builds the values `user_id`, `env_file_path`, `company_id` and optionally `os_group`, and creates the row when none exists. Creation and update both go through the model constraints (service account, path prefix, OS group), which run at the following `flush_all`, before any file is touched, so a refused registration cannot leave a half-written key file. The key itself is made afterwards by `_rotate_key_and_write_file`. Tests: `test_register_daemon_api`.

### `DaemonKeyRegistry._write_secure_env_file(self, path, login, key, group=None)` [@ANCHOR: COMM_write_secure_env_file_logic]

Writes a daemon's credentials to its key file. Arguments: `path` (resolved with `realpath`; must start with `/opt/hams/etc/keys/` or a `UserError` is raised, whatever the registry constraint said), `login`, `key`, and an optional `hamsd_<family>` `group`. Directories are created at `0700` (the key root is kept at `0710`; a family directory is handed to its group at `0750`). The content (`ODOO_RPC_LOGIN`, `ODOO_RPC_KEY`) goes to a fresh temporary file made `0600` by this process and is renamed over the target, so a pre-existing file with loose permissions can never receive a live key and a failed write leaves the old key intact; with a group the file becomes `0640` with that group. Returns nothing; permission and OS errors are logged and re-raised. Tests: `test_register_daemon_api`.

### `ir_cron_rotate_daemon_keys` scheduled action [@ANCHOR: COMM_cron_rotation_trigger]

The scheduled action, in `data/cron.xml` (`noupdate`), that starts rotation. It runs as the manager service account once a day (the first call a day after install) and executes `model._cron_rotate_all_keys()`. It is a daily check, not a rotation every day: `_cron_rotate_all_keys` rotates only registries older than the 59-day threshold, and the cron re-triggers itself while a full batch of 10 was found. Being `noupdate`, an operator's edit of its schedule survives module upgrades. Tests: `tests/test_key_registry.py` `test_cron_rotate_all_keys`.

### `_rotate_key_and_write_file`: revoke the old keys [@ANCHOR: COMM_revoke_old_keys_logic]

The first half of a rotation. For this registry's service account it deletes the `res.users.apikeys` rows named `<daemon name>_key` (at most 100, or those from the batch's prefetched set after an `exists()` check, because an earlier registry in the run may already have deleted its own). Only keys with that daemon's name are removed, so another daemon sharing the account keeps its key. Deleting is the revocation: the old key stops working at once, which is why daemons must re-read their key file after an HTTP 401. Tests: `test_cron_rotate_all_keys`, `test_key_ownership`.

### `_rotate_key_and_write_file`: mint and write the new key [@ANCHOR: COMM_generate_new_key_logic]

The second half of a rotation. It calls `_mint_key(<daemon name>_key)`, which creates an RPC API key owned by the daemon's service account (not the superuser) that expires `_lifetime_days()` days from now (90 by default), then `_write_secure_env_file` stores `ODOO_RPC_LOGIN` and `ODOO_RPC_KEY` in the registry's file (group-readable only if an `os_group` is set), and finally `last_rotated` is set to now. Before this the account's usage group is re-granted and archived or system accounts are refused. A failure of the write propagates and leaves `last_rotated` unchanged. Tests: `test_key_ownership`, `test_cron_rotate_all_keys`.
