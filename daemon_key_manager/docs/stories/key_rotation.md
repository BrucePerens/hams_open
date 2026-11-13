# Story: Automated 60-Day Key Rotation

As a **System Administrator**,
I want the **API keys for all registered daemons to rotate automatically**,
so that the security risk of a leaked key is limited in time and I don't have to perform manual rotations.

## Scenario: Periodic Maintenance

1.  Odoo's scheduled actions runner (cron) triggers the `ir_cron_rotate_daemon_keys` job every day.

2.  The job identifies all daemons whose keys were last rotated more than 59 days ago.
3.  For each eligible daemon, it:
    - Revokes the existing API key.

    - Generates a fresh 90-day API key.

    - Overwrites the existing `.env` file with the new key [@ANCHOR: COMM_write_secure_env_file_logic].
    - Updates the `last_rotated` timestamp.
4.  The external daemon, upon its next JSON-RPC call, may receive an `AccessError`.
5.  The daemon's error handler re-reads the `.env` file, acquires the new key, and retries the request successfully.

## Manual Rotation

If a specific daemon's credentials are suspected of being compromised, a manager can manually trigger a rotation for that specific daemon via the "Rotate Key" button on the registry form.

## Safety Safeguards

The system strictly prevents key rotation for service accounts that have been archived or disabled to prevent accidental reactivation of unauthorized access.

## Security Benefits
- Even if a backup of the `.env` file is stolen, the key will expire and be revoked within 60 days.
- The 90-day expiration on the Odoo side provides a 30-day buffer for the rotation to succeed.

## Scenario: A Daemon on Another Machine (Remote Self-Rotation)

Some daemons run on a different machine than Odoo (for example a sync daemon given a non-datacenter
egress path). Odoo cannot write that machine's key file, and the ordinary rotation revokes the old key at
once, so the daemon would be locked out at the first rotation after its key was installed. For these,
a manager sets **Remote Self-Rotation** on the registry:

1.  The cron, Force Provision All and Rotate Key all skip or refuse the registry
   .
2.  At the start of each run the daemon calls `rotate_own_key(daemon_name, current_key)` over JSON-2
    with its current key.
3.  Once the key is more than 59 days old, Odoo mints a new key and returns it, leaving the old key valid
   .
4.  The daemon writes the new key to its own key file and calls again, presenting the new key. Odoo then
    revokes the old key, writes the new one to `env_file_path` on the Odoo host too, and records the
    rotation.

A lost reply at either step leaves the daemon with a key that still works; the next run finishes the job.
Only a key belonging to that registry can rotate it, not another key of the same service account.

### Longer-Lived Keys for a Remote Daemon

A manager may set **Key Lifetime (days)** on a remote self-rotating registry, from 90 up to 400 days, for a daemon whose operator does not want it to depend on a rotation window (the FCC ULS sync on a non-datacenter machine is the first: at least one year). It applies to keys the registry mints from then on, and the daemon is due to rotate 31 days before its key expires, the same margin a 90-day key has. The **Apply Key Lifetime** button extends the key the daemon holds today to the new lifetime, counted from now. It never shortens a key, touches nothing when the row uses the default lifetime, leaves the key string unchanged (so the daemon needs to do nothing), and grants the service account the long-lived key-duration group that allows the longer expiry. Only a Daemon Key Manager may use it [@ANCHOR: COMM_action_apply_key_lifetime].

### A Manager Can See the Keys of Service Accounts

The Daemon Key Manager group implies the ordinary internal-user group, whose record rule shows each user only their own API keys. Without a rule of its own, every search the manager service made for a service account's keys came back empty, so no rotation ever revoked an old key and a remote daemon's confirmation revoked "0 old keys". An additional record rule lets the Daemon Key Manager group read and revoke API keys, but only the keys of service accounts; group rules combine, so no one else's access widens [@ANCHOR: COMM_manager_apikeys_of_service_accounts_rule].

Note: the "30-day buffer" below does not apply to keys a daemon cannot re-read after a rotation. The
ordinary rotation revokes the old key in the same transaction as it mints the new one.

## Entries

### `ir_cron_rotate_daemon_keys` scheduled action [@ANCHOR: COMM_cron_rotation_trigger]

The scheduled action, in `data/cron.xml` (`noupdate`), that starts rotation. It runs as the manager service account once a day (the first call a day after install) and executes `model._cron_rotate_all_keys()`. It is a daily check, not a rotation every day: `_cron_rotate_all_keys` rotates only registries older than the 59-day threshold, and the cron re-triggers itself while a full batch of 10 was found. Being `noupdate`, an operator's edit of its schedule survives module upgrades. Tests: `tests/test_key_registry.py` `test_cron_rotate_all_keys`.

### `DaemonKeyRegistry._cron_rotate_all_keys(self)` [@ANCHOR: COMM_cron_rotation_logic]

The rotation worker run by the cron. No arguments; returns nothing. It runs as the manager service account, selects up to 10 registries that are not remote self-rotating, whose service account is active and whose `last_rotated` is empty or older than 59 days, oldest first, and prefetches their old API keys in one query. Each is rotated and committed on its own; a managed failure (OS, user, validation or access error) rolls back that one, is logged, and leaves `last_rotated` untouched so it is retried on the next run instead of being exempted for 59 days. When a full batch of 10 was found it triggers the cron again for the rest. Tests: `test_cron_rotate_all_keys`.

### `DaemonKeyRegistry._mark_rotation_failed(self, reg)` [@ANCHOR: daemon_key_manager:mark_rotation_failed]

Records that the cron just failed to rotate one registry. Argument `reg`: the registry row. Sets `last_rotation_failed_at` to now and commits at once, in its own write, so the mark survives the rollback of the failed rotation. Returns nothing. The cron orders its batch by this field (never-failed first), so a registry that fails on every run goes to the back of the queue instead of filling the batch of 10 for ever. Called by `_cron_rotate_all_keys` for a managed failure. Tests: `test_failing_registries_do_not_block_a_good_one`.

### `_rotate_key_and_write_file`: revoke the old keys [@ANCHOR: COMM_revoke_old_keys_logic]

The first half of a rotation. For this registry's service account it deletes the `res.users.apikeys` rows named `<daemon name>_key` (at most 100, or those from the batch's prefetched set after an `exists()` check, because an earlier registry in the run may already have deleted its own). Only keys with that daemon's name are removed, so another daemon sharing the account keeps its key. Deleting is the revocation: the old key stops working at once, which is why daemons must re-read their key file after an HTTP 401. Tests: `test_cron_rotate_all_keys`, `test_key_ownership`.

### `_rotate_key_and_write_file`: mint and write the new key [@ANCHOR: COMM_generate_new_key_logic]

The second half of a rotation. It calls `_mint_key(<daemon name>_key)`, which creates an RPC API key owned by the daemon's service account (not the superuser) that expires `_lifetime_days()` days from now (90 by default), then `_write_secure_env_file` stores `ODOO_RPC_LOGIN` and `ODOO_RPC_KEY` in the registry's file (group-readable only if an `os_group` is set), and finally `last_rotated` is set to now. Before this the account's usage group is re-granted and archived or system accounts are refused. A failure of the write propagates and leaves `last_rotated` unchanged. Tests: `test_key_ownership`, `test_cron_rotate_all_keys`.

### `DaemonKeyRegistry.action_rotate_key(self)` [@ANCHOR: COMM_action_rotate_key_api]

The "Rotate Key" button on the registry form: manually rotates one daemon's key now. Exactly one record (`ensure_one`); only a Daemon Key Manager may call it (`AccessError` otherwise). It runs `_rotate_key_and_write_file` in the registry's company, which revokes the old key, mints a new one, rewrites the key file and sets `last_rotated`, and returns a success notification that reloads the view. A registry set to Remote Self-Rotation, or one whose account is archived or a system account, is refused with a `UserError`. Exposed to RPC with `@public_rpc`. Tests: `test_action_rotate_key`.

### `DaemonKeyRegistry._assert_account_may_hold_key(self)` [@ANCHOR: COMM_rotation_safety_archived_user]

Refuses to mint a key for the wrong kind of account. An archived service account raises a `UserError` ("Cannot rotate key for archived service account") so a rotation can never quietly give a disabled account working credentials again; the `__system__` user and any member of `base.group_system` are also refused, because such an account must never have an RPC key. Exactly one record; no side effects other than the error. It runs at the start of every rotation, after the Remote Self-Rotation check. Tests: `test_rotation_safety_archived_user`.

### `_rotate_key_and_write_file`: remote self-rotating registries are refused [@ANCHOR: COMM_remote_self_rotation_excluded_from_local_rotation]

A registry with Remote Self-Rotation set is for a daemon on another machine whose key file Odoo cannot write. The local rotation path revokes the old key and writes the new one to a file on the Odoo host, which would strand that daemon on a revoked key, so `_rotate_key_and_write_file` raises a `UserError` that names the daemon and says to clear the flag first. The nightly cron and Force Provision All filter such registries out of their searches, and the Rotate Key button hits this refusal. The daemon rotates through `rotate_own_key` instead. Tests: `tests/test_remote_self_rotation.py`.

### `DaemonKeyRegistry.rotate_own_key(self, daemon_name, current_key) -> dict` [@ANCHOR: COMM_rotate_own_key_api]

Two-phase remote self-rotation, called over JSON-2 by a daemon with its current key. Arguments: `daemon_name` and `current_key`, both strings. The registry must carry this daemon's name, belong to the calling service account and have Remote Self-Rotation set; the presented key must be a live key of that registry (checked by index and hash). Any failure gives one `AccessError` ("not available for daemon ...") so it does not reveal which registries exist. A key equal to the pending key confirms the rotation, anything else asks for a new key. Returns `{status: issued, login, key}`, `not_due` with `next_rotation`, or `confirmed`. Exposed with `@public_rpc`. Tests: `test_two_phase_rotation_revokes_nothing_until_the_new_key_is_proven` in `tests/test_remote_self_rotation.py`.

### `DaemonKeyRegistry._issue_remote_rotation(self)` [@ANCHOR: COMM_rotate_own_key_issue]

Step 1 of remote rotation. If `last_rotated` is set and the key is younger than the rotation age (the lifetime minus 31 days, 59 for a 90-day key), it returns `not_due` with the due time and changes nothing. Otherwise it deletes any earlier unconfirmed pending key (its reply never reached the daemon), re-grants the usage group, mints a new key, records its row id as `pending_key_id`, logs a warning, and returns the new key with the login. The old key is left valid, so a lost reply leaves the daemon able to retry. Tests: `tests/test_remote_self_rotation.py`.

### `DaemonKeyRegistry._confirm_remote_rotation(self, new_key)` [@ANCHOR: COMM_rotate_own_key_confirm]

Step 2 of remote rotation, reached when the daemon calls again presenting the pending key, which proves it holds it. Argument `new_key`: the presented key. It deletes every other key named `<daemon>_key` of the service account (at most 100), writes the new key to `env_file_path` on the Odoo host too with the registry's group, clears `pending_key_id`, sets `last_rotated`, logs a warning with the number of keys revoked, and returns `{status: confirmed}`. A lost reply is harmless: the next call with the new key repeats this step. Tests: `tests/test_remote_self_rotation.py`.
