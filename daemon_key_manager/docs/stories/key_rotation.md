# Story: Automated 60-Day Key Rotation

As a **System Administrator**,
I want the **API keys for all registered daemons to rotate automatically**,
so that the security risk of a leaked key is limited in time and I don't have to perform manual rotations.

## Scenario: Periodic Maintenance

1.  Odoo's scheduled actions runner (cron) triggers the `ir_cron_rotate_daemon_keys` job every day [@ANCHOR: COMM_cron_rotation_trigger].

2.  The job identifies all daemons whose keys were last rotated more than 59 days ago [@ANCHOR: COMM_cron_rotation_logic].
3.  For each eligible daemon, it:
    - Revokes the existing API key [@ANCHOR: COMM_revoke_old_keys_logic].

    - Generates a fresh 90-day API key [@ANCHOR: COMM_generate_new_key_logic].

    - Overwrites the existing `.env` file with the new key [@ANCHOR: COMM_write_secure_env_file_logic].
    - Updates the `last_rotated` timestamp.
4.  The external daemon, upon its next JSON-RPC call, may receive an `AccessError`.
5.  The daemon's error handler re-reads the `.env` file, acquires the new key, and retries the request successfully.

## Manual Rotation

If a specific daemon's credentials are suspected of being compromised, a manager can manually trigger a rotation for that specific daemon via the "Rotate Key" button on the registry form [@ANCHOR: COMM_action_rotate_key_api].

## Safety Safeguards

The system strictly prevents key rotation for service accounts that have been archived or disabled to prevent accidental reactivation of unauthorized access [@ANCHOR: COMM_rotation_safety_archived_user].

## Security Benefits
- Even if a backup of the `.env` file is stolen, the key will expire and be revoked within 60 days.
- The 90-day expiration on the Odoo side provides a 30-day buffer for the rotation to succeed.

## Scenario: A Daemon on Another Machine (Remote Self-Rotation)

Some daemons run on a different machine than Odoo (for example a sync daemon given a non-datacenter
egress path). Odoo cannot write that machine's key file, and the ordinary rotation revokes the old key at
once, so the daemon would be locked out at the first rotation after its key was installed. For these,
a manager sets **Remote Self-Rotation** on the registry:

1.  The cron, Force Provision All and Rotate Key all skip or refuse the registry
    [@ANCHOR: COMM_remote_self_rotation_excluded_from_local_rotation].
2.  At the start of each run the daemon calls `rotate_own_key(daemon_name, current_key)` over JSON-2
    with its current key [@ANCHOR: COMM_rotate_own_key_api].
3.  Once the key is more than 59 days old, Odoo mints a new key and returns it, leaving the old key valid
    [@ANCHOR: COMM_rotate_own_key_issue].
4.  The daemon writes the new key to its own key file and calls again, presenting the new key. Odoo then
    revokes the old key, writes the new one to `env_file_path` on the Odoo host too, and records the
    rotation [@ANCHOR: COMM_rotate_own_key_confirm].

A lost reply at either step leaves the daemon with a key that still works; the next run finishes the job.
Only a key belonging to that registry can rotate it, not another key of the same service account.

### Longer-Lived Keys for a Remote Daemon

A manager may set **Key Lifetime (days)** on a remote self-rotating registry, from 90 up to 400 days, for a daemon whose operator does not want it to depend on a rotation window (the FCC ULS sync on a non-datacenter machine is the first: at least one year). It applies to keys the registry mints from then on, and the daemon is due to rotate 31 days before its key expires, the same margin a 90-day key has. The **Apply Key Lifetime** button extends the key the daemon holds today to the new lifetime, counted from now. It never shortens a key, touches nothing when the row uses the default lifetime, leaves the key string unchanged (so the daemon needs to do nothing), and grants the service account the long-lived key-duration group that allows the longer expiry. Only a Daemon Key Manager may use it [@ANCHOR: COMM_action_apply_key_lifetime].

### A Manager Can See the Keys of Service Accounts

The Daemon Key Manager group implies the ordinary internal-user group, whose record rule shows each user only their own API keys. Without a rule of its own, every search the manager service made for a service account's keys came back empty, so no rotation ever revoked an old key and a remote daemon's confirmation revoked "0 old keys". An additional record rule lets the Daemon Key Manager group read and revoke API keys, but only the keys of service accounts; group rules combine, so no one else's access widens [@ANCHOR: COMM_manager_apikeys_of_service_accounts_rule].

Note: the "30-day buffer" below does not apply to keys a daemon cannot re-read after a rotation. The
ordinary rotation revokes the old key in the same transaction as it mints the new one.
